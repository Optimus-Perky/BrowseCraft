//! Single sign-on from domain PCs: HTTP Negotiate (Kerberos), checked with MIT Kerberos (GSSAPI).
//!
//! The browser on a domain PC asks the domain for a ticket to HTTP/<this server's DNS name> and
//! sends it; the server checks it with the keytab of the account that has that SPN. Nothing is
//! asked of the user. Settings:
//!
//!   KRB5_KTNAME   the keytab (e.g. /data/browsecraft.keytab, made with ktpass on a DC)
//!
//! The ticket says who the user is (NAME@REALM); ldap.rs then reads their entry (needs the
//! BROWSECRAFT_LDAP_BIND_* settings, as there is no user password to bind with).

use std::ffi::c_void;
use std::ptr;

type OmUint32 = u32;
type GssCtx = *mut c_void;
type GssName = *mut c_void;
type GssCred = *mut c_void;

#[repr(C)]
struct GssBuffer {
    length: usize,
    value: *mut c_void,
}

const GSS_S_COMPLETE: OmUint32 = 0;
const GSS_S_CONTINUE_NEEDED: OmUint32 = 1;
const GSS_C_GSS_CODE: i32 = 1;
const GSS_C_MECH_CODE: i32 = 2;

#[link(name = "gssapi_krb5")]
unsafe extern "C" {
    fn gss_accept_sec_context(
        minor: *mut OmUint32,
        context: *mut GssCtx,
        acceptor_cred: GssCred,
        input_token: *mut GssBuffer,
        channel_bindings: *mut c_void,
        src_name: *mut GssName,
        mech_type: *mut *mut c_void,
        output_token: *mut GssBuffer,
        ret_flags: *mut OmUint32,
        time_rec: *mut OmUint32,
        delegated_cred: *mut GssCred,
    ) -> OmUint32;
    fn gss_display_name(minor: *mut OmUint32, name: GssName, out: *mut GssBuffer, out_type: *mut *mut c_void) -> OmUint32;
    fn gss_display_status(
        minor: *mut OmUint32,
        status: OmUint32,
        status_type: i32,
        mech_type: *mut c_void,
        message_context: *mut OmUint32,
        out: *mut GssBuffer,
    ) -> OmUint32;
    fn gss_release_buffer(minor: *mut OmUint32, buffer: *mut GssBuffer) -> OmUint32;
    fn gss_release_name(minor: *mut OmUint32, name: *mut GssName) -> OmUint32;
    fn gss_delete_sec_context(minor: *mut OmUint32, context: *mut GssCtx, out: *mut GssBuffer) -> OmUint32;
}

/// Whether single sign-on is set up (a keytab is named and there).
pub fn enabled() -> bool {
    std::env::var("KRB5_KTNAME").ok().map(|k| k.trim_start_matches("FILE:").to_string()).is_some_and(|k| std::path::Path::new(&k).is_file())
}

/// The ticket checked: who it is, and the token for the reply (mutual authentication), if any.
#[derive(Debug)]
pub struct Accepted {
    pub principal: String,
    pub reply: Vec<u8>,
}

fn take(buf: &mut GssBuffer) -> Vec<u8> {
    let out = if buf.value.is_null() || buf.length == 0 {
        Vec::new()
    } else {
        // SAFETY: GSSAPI filled `buf` with `length` bytes at `value`.
        unsafe { std::slice::from_raw_parts(buf.value as *const u8, buf.length) }.to_vec()
    };
    let mut minor = 0;
    // SAFETY: a buffer GSSAPI allocated (or an empty one), released once.
    unsafe { gss_release_buffer(&mut minor, buf) };
    out
}

fn status_text(code: OmUint32, kind: i32) -> String {
    let mut parts = Vec::new();
    let mut ctx: OmUint32 = 0;
    loop {
        let (mut minor, mut buf) = (0, GssBuffer { length: 0, value: ptr::null_mut() });
        // SAFETY: plain out-parameters; the buffer is released by take().
        let major = unsafe { gss_display_status(&mut minor, code, kind, ptr::null_mut(), &mut ctx, &mut buf) };
        if major != GSS_S_COMPLETE {
            break;
        }
        parts.push(String::from_utf8_lossy(&take(&mut buf)).into_owned());
        if ctx == 0 {
            break;
        }
    }
    parts.join("; ")
}

/// Checks a Negotiate token (SPNEGO or bare Kerberos) from a browser. Blocking: run it off the
/// async threads. One round trip only: Kerberos needs no more, and NTLM is not accepted.
pub fn accept(token: &[u8]) -> Result<Accepted, String> {
    let mut minor: OmUint32 = 0;
    let mut ctx: GssCtx = ptr::null_mut();
    let mut name: GssName = ptr::null_mut();
    let mut input = GssBuffer { length: token.len(), value: token.as_ptr() as *mut c_void };
    let mut output = GssBuffer { length: 0, value: ptr::null_mut() };
    // SAFETY: GSSAPI only reads `input` (which `token` outlives); the rest are out-parameters,
    // released below. No acceptor credential = any key in the default keytab (KRB5_KTNAME).
    let major = unsafe {
        gss_accept_sec_context(
            &mut minor,
            &mut ctx,
            ptr::null_mut(),
            &mut input,
            ptr::null_mut(),
            &mut name,
            ptr::null_mut(),
            &mut output,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    let reply = take(&mut output);
    let result = if major == GSS_S_COMPLETE {
        let mut shown = GssBuffer { length: 0, value: ptr::null_mut() };
        let mut m2 = 0;
        // SAFETY: `name` was set by a successful accept; `shown` is released by take().
        let r = unsafe { gss_display_name(&mut m2, name, &mut shown, ptr::null_mut()) };
        if r == GSS_S_COMPLETE {
            Ok(Accepted { principal: String::from_utf8_lossy(&take(&mut shown)).into_owned(), reply })
        } else {
            Err(format!("could not read the ticket's name: {}", status_text(r, GSS_C_GSS_CODE)))
        }
    } else if major == GSS_S_CONTINUE_NEEDED {
        Err("the browser offered NTLM, not Kerberos (is the server's address a DNS name with an SPN?)".into())
    } else {
        let mech = status_text(minor, GSS_C_MECH_CODE);
        Err(format!("{}{}", status_text(major, GSS_C_GSS_CODE), if mech.is_empty() { String::new() } else { format!(" ({mech})") }))
    };
    let mut m3 = 0;
    // SAFETY: release what accept made (null handles are allowed).
    unsafe {
        if !name.is_null() {
            gss_release_name(&mut m3, &mut name);
        }
        if !ctx.is_null() {
            gss_delete_sec_context(&mut m3, &mut ctx, ptr::null_mut());
        }
    }
    result
}

/// "jo.bloggs@EXAMPLE.LAN" -> "jo.bloggs", when the realm is the domain's.
pub fn account_in(principal: &str, domain: &str) -> Option<String> {
    let (name, realm) = principal.rsplit_once('@')?;
    if !realm.eq_ignore_ascii_case(domain) || name.is_empty() || name.contains('/') {
        return None; // another realm, or a service (HTTP/host) rather than a person
    }
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principals() {
        assert_eq!(account_in("jo.bloggs@EXAMPLE.LAN", "example.lan").as_deref(), Some("jo.bloggs"));
        assert_eq!(account_in("mark@OTHER.LAN", "example.lan"), None);
        assert_eq!(account_in("HTTP/web@EXAMPLE.LAN", "example.lan"), None);
        assert_eq!(account_in("nobody", "example.lan"), None);
    }

    #[test]
    fn rubbish_is_refused() {
        assert!(accept(b"not a ticket").is_err());
    }
}

//! Signing in with a domain account (Active Directory or Samba AD), over LDAPS.
//!
//! The password is checked by binding to a domain controller as the user; nothing is stored and
//! no service account is needed. Bound as the user, the server reads their own entry: enabled or
//! not, and whether they are in the admin group (nested groups count). Settings:
//!
//!   BROWSECRAFT_LDAP_URL          ldaps://dc01.example.lan, ldaps://dc02.example.lan (tried in turn)
//!   BROWSECRAFT_LDAP_DOMAIN       example.lan (user names are tried as NAME@example.lan)
//!   BROWSECRAFT_LDAP_CA_FILE      PEM file with the DCs' certificates, or the CA that issued them
//!   BROWSECRAFT_LDAP_BASE_DN      default: from the domain (DC=example,DC=lan)
//!   BROWSECRAFT_LDAP_ADMIN_GROUP  default "Domain Admins"; "none" for no domain admins
//!   BROWSECRAFT_LDAP_USER_GROUP   only members may sign in (default: any enabled user)
//!
//! Single sign-on (kerberos.rs) has no password to bind with, so it reads the user's entry as a
//! service account (any domain user; the one whose keytab the server uses will do):
//!
//!   BROWSECRAFT_LDAP_BIND_USER           NAME, DOMAIN\NAME or NAME@domain
//!   BROWSECRAFT_LDAP_BIND_PASSWORD_FILE  a file holding its password (keep it 0600)

use std::sync::Arc;
use std::time::Duration;

use ldap3::exop::{WhoAmI, WhoAmIResp};
use ldap3::{Ldap, LdapConnAsync, LdapConnSettings, Scope, SearchEntry, ldap_escape};
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;

pub struct Config {
    urls: Vec<String>,
    domain: String,
    base_dn: String,
    admin_group: Option<String>,
    user_group: Option<String>,
    /// The service account for single sign-on look-ups (UPN, password).
    service: Option<(String, String)>,
    tls: Arc<rustls::ClientConfig>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DomainUser {
    pub name: String,
    /// Their full name (displayName; else given name and surname; else the user name).
    pub display_name: String,
    /// Their photo in the directory (thumbnailPhoto), if there is one.
    pub photo: Option<Vec<u8>>,
    pub is_admin: bool,
    /// Every group the user is in, nested groups included (sAMAccountName), for the extension store.
    pub groups: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    WrongPassword,
    /// The password was right but the account may not sign in (with the reason to show).
    NotAllowed(String),
    /// No domain controller answered properly.
    Unavailable(String),
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

impl Config {
    /// The settings from the environment; None when domain sign-in is not set up.
    pub fn from_env() -> Result<Option<Config>, String> {
        let Some(urls) = var("BROWSECRAFT_LDAP_URL") else { return Ok(None) };
        let urls: Vec<String> = urls.split([',', ' ']).filter(|u| !u.is_empty()).map(str::to_string).collect();
        if let Some(u) = urls.iter().find(|u| !u.to_ascii_lowercase().starts_with("ldaps://")) {
            return Err(format!("BROWSECRAFT_LDAP_URL: {u} is not ldaps:// (passwords must not cross the network in clear)"));
        }
        let domain = var("BROWSECRAFT_LDAP_DOMAIN").ok_or("BROWSECRAFT_LDAP_DOMAIN is needed with BROWSECRAFT_LDAP_URL")?.to_ascii_lowercase();
        let base_dn = var("BROWSECRAFT_LDAP_BASE_DN").unwrap_or_else(|| domain.split('.').map(|p| format!("DC={p}")).collect::<Vec<_>>().join(","));
        let admin_group = match var("BROWSECRAFT_LDAP_ADMIN_GROUP") {
            Some(g) if g.eq_ignore_ascii_case("none") => None,
            Some(g) => Some(g),
            None => Some("Domain Admins".to_string()),
        };
        let ca = var("BROWSECRAFT_LDAP_CA_FILE").ok_or("BROWSECRAFT_LDAP_CA_FILE is needed: the DCs' certificates (or their CA) as PEM")?;
        let mut roots = rustls::RootCertStore::empty();
        for cert in CertificateDer::pem_file_iter(&ca).map_err(|e| format!("{ca}: {e}"))? {
            roots.add(cert.map_err(|e| format!("{ca}: {e}"))?).map_err(|e| format!("{ca}: {e}"))?;
        }
        if roots.is_empty() {
            return Err(format!("{ca}: no certificates in it"));
        }
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let mut cfg = Config { urls, domain, base_dn, admin_group, user_group: var("BROWSECRAFT_LDAP_USER_GROUP"), service: None, tls: Arc::new(tls) };
        if let Some(user) = var("BROWSECRAFT_LDAP_BIND_USER") {
            let file = var("BROWSECRAFT_LDAP_BIND_PASSWORD_FILE").ok_or("BROWSECRAFT_LDAP_BIND_PASSWORD_FILE is needed with BROWSECRAFT_LDAP_BIND_USER")?;
            let password = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?.trim_end_matches(['\r', '\n']).to_string();
            if password.is_empty() {
                return Err(format!("{file}: empty"));
            }
            let upn = cfg.bind_name(&user).ok_or("BROWSECRAFT_LDAP_BIND_USER: not a user name")?;
            cfg.service = Some((upn, password));
        }
        Ok(Some(cfg))
    }

    /// The domain (DNS form), which is also the Kerberos realm.
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// Whether entries can be read without the user's password (for single sign-on).
    pub fn can_look_up(&self) -> bool {
        self.service.is_some()
    }

    /// One line for logs and the admin page.
    pub fn describe(&self) -> String {
        let hosts: Vec<&str> = self.urls.iter().map(|u| u.trim_start_matches("ldaps://").trim_start_matches("LDAPS://").trim_end_matches('/')).collect();
        format!(
            "{} ({}){}{}",
            self.domain,
            hosts.join(", "),
            self.user_group.as_ref().map(|g| format!("; members of {g} only")).unwrap_or_default(),
            self.admin_group.as_ref().map(|g| format!("; {g} are admins")).unwrap_or_default()
        ) + if self.service.is_some() { "; look-ups as a service account" } else { "" }
    }

    /// The account to bind as for what the user typed: NAME, DOMAIN\NAME or NAME@domain.
    fn bind_name(&self, login: &str) -> Option<String> {
        let login = login.trim();
        let name = login.rsplit_once('\\').map_or(login, |(_, n)| n);
        if name.is_empty() || name.len() > 256 || name.chars().any(|c| c.is_control()) {
            return None;
        }
        Some(if name.contains('@') { name.to_string() } else { format!("{name}@{}", self.domain) })
    }
}

/// Checks a domain user name and password.
pub async fn authenticate(cfg: &Config, login: &str, password: &str) -> Result<DomainUser, Refused> {
    // An empty password would be an anonymous bind, which "succeeds".
    if password.is_empty() {
        return Err(Refused::WrongPassword);
    }
    let Some(upn) = cfg.bind_name(login) else { return Err(Refused::WrongPassword) };
    let mut last = String::from("no domain controllers set");
    for url in &cfg.urls {
        match tokio::time::timeout(Duration::from_secs(15), try_dc(cfg, url, &upn, password)).await {
            Ok(Err(Refused::Unavailable(e))) => last = format!("{url}: {e}"),
            Ok(r) => return r,
            Err(_) => last = format!("{url}: no answer in 15 seconds"),
        }
    }
    Err(Refused::Unavailable(last))
}

/// Reads a domain user's entry as the service account (single sign-on: the ticket has already
/// shown who they are). `sam` is their account name.
pub async fn look_up(cfg: &Config, sam: &str) -> Result<DomainUser, Refused> {
    let Some((upn, password)) = &cfg.service else {
        return Err(Refused::Unavailable("BROWSECRAFT_LDAP_BIND_USER is not set".into()));
    };
    let mut last = String::from("no domain controllers set");
    for url in &cfg.urls {
        let attempt = async {
            let settings = LdapConnSettings::new().set_conn_timeout(Duration::from_secs(5)).set_config(cfg.tls.clone());
            let (conn, mut ldap) = LdapConnAsync::with_settings(settings, url).await.map_err(unavailable)?;
            ldap3::drive!(conn);
            let bind = ldap.simple_bind(upn, password).await.map_err(unavailable)?;
            let r = if bind.rc == 0 {
                entry_for(cfg, &mut ldap, sam).await
            } else {
                Err(Refused::Unavailable(format!("the service account could not sign in: {} {}", bind.rc, bind.text)))
            };
            let _ = ldap.unbind().await;
            r
        };
        match tokio::time::timeout(Duration::from_secs(15), attempt).await {
            Ok(Err(Refused::Unavailable(e))) => last = format!("{url}: {e}"),
            Ok(r) => return r,
            Err(_) => last = format!("{url}: no answer in 15 seconds"),
        }
    }
    Err(Refused::Unavailable(last))
}

fn unavailable(e: impl std::fmt::Display) -> Refused {
    Refused::Unavailable(e.to_string())
}

async fn try_dc(cfg: &Config, url: &str, upn: &str, password: &str) -> Result<DomainUser, Refused> {
    let settings = LdapConnSettings::new().set_conn_timeout(Duration::from_secs(5)).set_config(cfg.tls.clone());
    let (conn, mut ldap) = LdapConnAsync::with_settings(settings, url).await.map_err(unavailable)?;
    ldap3::drive!(conn);
    let r = check(cfg, &mut ldap, upn, password).await;
    let _ = ldap.unbind().await;
    r
}

async fn check(cfg: &Config, ldap: &mut Ldap, upn: &str, password: &str) -> Result<DomainUser, Refused> {
    let bind = ldap.simple_bind(upn, password).await.map_err(unavailable)?;
    match bind.rc {
        0 => {}
        // invalidCredentials; AD puts the reason in the text as "data NNN".
        49 => {
            let t = bind.text.to_ascii_lowercase();
            return Err(if t.contains("data 533") {
                Refused::NotAllowed("that domain account is disabled".into())
            } else if t.contains("data 775") {
                Refused::NotAllowed("that domain account is locked out".into())
            } else if t.contains("data 532") || t.contains("data 773") {
                Refused::NotAllowed("that domain password has expired or must be changed: change it on a domain PC, then sign in".into())
            } else if t.contains("data 701") {
                Refused::NotAllowed("that domain account has expired".into())
            } else {
                Refused::WrongPassword
            });
        }
        _ => return Err(Refused::Unavailable(format!("bind: {} {}", bind.rc, bind.text))),
    }

    // Who the bind really was (DOMAIN\name on AD), then that account's entry.
    let (exop, _) = ldap.extended(WhoAmI).await.and_then(|r| r.success()).map_err(unavailable)?;
    let who: WhoAmIResp = exop.parse();
    let sam = who.authzid.trim_start_matches("u:").rsplit_once('\\').map_or(who.authzid.as_str(), |(_, n)| n).to_string();
    if sam.is_empty() {
        return Err(Refused::Unavailable(format!("the server did not say who {upn} is")));
    }
    entry_for(cfg, ldap, &sam).await
}

/// The checks and details for account `sam`, read over a bound connection.
async fn entry_for(cfg: &Config, ldap: &mut Ldap, sam: &str) -> Result<DomainUser, Refused> {
    let filter = format!("(&(objectCategory=person)(objectClass=user)(sAMAccountName={}))", ldap_escape(sam));
    let (found, _) =
        ldap.search(&cfg.base_dn, Scope::Subtree, &filter, vec!["sAMAccountName", "userAccountControl", "displayName", "givenName", "sn", "thumbnailPhoto"])
            .await
            .and_then(|r| r.success())
            .map_err(unavailable)?;
    let Some(entry) = found.into_iter().next().map(SearchEntry::construct) else {
        return Err(Refused::Unavailable(format!("could not find {sam} under {}", cfg.base_dn)));
    };
    let first = |a: &str| entry.attrs.get(a).and_then(|v| v.first()).cloned();
    let uac: u32 = first("userAccountControl").and_then(|v| v.parse().ok()).unwrap_or(0);
    if uac & 2 != 0 {
        return Err(Refused::NotAllowed("that domain account is disabled".into()));
    }
    if let Some(g) = &cfg.user_group
        && !member_of(cfg, ldap, &entry.dn, g).await?
    {
        return Err(Refused::NotAllowed(format!("only members of {g} may use AltBrowse sync")));
    }
    let is_admin = match &cfg.admin_group {
        Some(g) => member_of(cfg, ldap, &entry.dn, g).await?,
        None => false,
    };
    let groups = groups_of(cfg, ldap, &entry.dn).await?;
    let name = first("sAMAccountName").unwrap_or_else(|| sam.to_string());
    let display_name = first("displayName")
        .filter(|n| !n.trim().is_empty())
        .or_else(|| {
            let both = format!("{} {}", first("givenName").unwrap_or_default(), first("sn").unwrap_or_default());
            Some(both.trim().to_string()).filter(|n| !n.is_empty())
        })
        .unwrap_or_else(|| name.clone());
    let photo = entry.bin_attrs.get("thumbnailPhoto").and_then(|v| v.first()).cloned().filter(|p| !p.is_empty());
    Ok(DomainUser { name, display_name, photo, is_admin, groups })
}

/// The groups `user_dn` is in, directly or through nested groups (LDAP_MATCHING_RULE_IN_CHAIN).
async fn groups_of(cfg: &Config, ldap: &mut Ldap, user_dn: &str) -> Result<Vec<String>, Refused> {
    let filter = format!("(&(objectCategory=group)(member:1.2.840.113556.1.4.1941:={}))", ldap_escape(user_dn));
    let (found, _) = ldap.search(&cfg.base_dn, Scope::Subtree, &filter, vec!["sAMAccountName"]).await.and_then(|r| r.success()).map_err(unavailable)?;
    let mut out: Vec<String> =
        found.into_iter().filter_map(|e| SearchEntry::construct(e).attrs.get("sAMAccountName").and_then(|v| v.first()).cloned()).collect();
    out.sort();
    Ok(out)
}

/// Whether `user_dn` is in `group` (a name or a DN), directly or through nested groups.
async fn member_of(cfg: &Config, ldap: &mut Ldap, user_dn: &str, group: &str) -> Result<bool, Refused> {
    let group_dn = if group.contains('=') {
        group.to_string()
    } else {
        let filter = format!("(&(objectCategory=group)(sAMAccountName={}))", ldap_escape(group));
        let (found, _) = ldap.search(&cfg.base_dn, Scope::Subtree, &filter, vec!["1.1"]).await.and_then(|r| r.success()).map_err(unavailable)?;
        match found.into_iter().next() {
            Some(e) => SearchEntry::construct(e).dn,
            None => return Ok(false),
        }
    };
    // LDAP_MATCHING_RULE_IN_CHAIN: membership through nested groups too (AD and Samba AD).
    let filter = format!("(memberOf:1.2.840.113556.1.4.1941:={})", ldap_escape(group_dn.as_str()));
    let (found, _) = ldap.search(user_dn, Scope::Base, &filter, vec!["1.1"]).await.and_then(|r| r.success()).map_err(unavailable)?;
    Ok(!found.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map(|b| b.with_root_certificates(rustls::RootCertStore::empty()).with_no_client_auth());
        let Ok(tls) = tls else { unreachable!() };
        Config {
            urls: vec!["ldaps://dc01.example.lan".into()],
            domain: "example.lan".into(),
            base_dn: "DC=example,DC=lan".into(),
            admin_group: Some("Domain Admins".into()),
            user_group: None,
            service: None,
            tls: Arc::new(tls),
        }
    }

    #[test]
    fn bind_names() {
        let c = cfg();
        assert_eq!(c.bind_name("jo.bloggs").as_deref(), Some("jo.bloggs@example.lan"));
        assert_eq!(c.bind_name("EXAMPLE\\jo.bloggs").as_deref(), Some("jo.bloggs@example.lan"));
        assert_eq!(c.bind_name(" mark@other.lan ").as_deref(), Some("mark@other.lan"));
        assert_eq!(c.bind_name(""), None);
        assert_eq!(c.bind_name("EXAMPLE\\"), None);
    }

    #[tokio::test]
    async fn empty_password_is_never_tried() {
        // An empty password would be an anonymous bind; it must not get as far as a DC.
        assert_eq!(authenticate(&cfg(), "anyone", "").await, Err(Refused::WrongPassword));
    }

    #[test]
    fn describes_itself() {
        assert_eq!(cfg().describe(), "example.lan (dc01.example.lan); Domain Admins are admins");
    }
}

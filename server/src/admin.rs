//! The server's own web pages (no JavaScript): first-run setup, an admin sign-in, and the
//! admin page (users, their devices, add / reset password / remove).

use axum::extract::{Form, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};

use crate::store;
use crate::web::{App, with_db};

const COOKIE: &str = "bc_admin";

pub(crate) fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

pub(crate) fn page(title: &str, body: &str) -> Response {
    let html = format!(
        "<!doctype html><html lang=en><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>{t} - BrowseCraft sync</title>\
<style>:root{{color-scheme:light dark;--a:#2f6f5e;--t:#1f2328;--d:#5b636b;--b:#f6f7f8;--c:#fff;--l:#d9dde1}}\
@media(prefers-color-scheme:dark){{:root{{--a:#5fb39b;--t:#e6e8ea;--d:#a3aab1;--b:#1c1f22;--c:#25292d;--l:#3a3f44}}}}\
body{{margin:0;background:var(--b);color:var(--t);font:15px/1.5 system-ui,sans-serif}}main{{max-width:46em;margin:3em auto;padding:0 1em}}\
.card{{background:var(--c);border:1px solid var(--l);border-radius:10px;padding:1.4em 1.8em;margin:1em 0}}h1{{margin:.2em 0}}\
label{{display:block;margin:.8em 0 0;font-weight:600}}input[type=text],input[type=password],input[type=search]{{display:block;width:100%;box-sizing:border-box;margin-top:.3em;padding:.5em .7em;border:1px solid var(--l);border-radius:6px;background:var(--b);color:var(--t);font:inherit}}\
button{{margin-top:1em;padding:.5em 1.2em;border:0;border-radius:6px;background:var(--a);color:#fff;font:inherit;font-weight:600;cursor:pointer}}button.plain{{background:none;color:var(--a);border:1px solid var(--l);margin:0}}\
table{{width:100%;border-collapse:collapse}}td,th{{text-align:left;padding:.45em .3em;border-bottom:1px solid var(--l);vertical-align:top}}.d{{color:var(--d)}}.err{{color:#c0392b}}.ok{{color:var(--a)}}\
form.inline{{display:inline}}</style><main>{body}</main></html>",
        t = esc(title)
    );
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

fn cookie_token(h: &HeaderMap) -> Option<String> {
    let c = h.get(header::COOKIE)?.to_str().ok()?;
    c.split(';').map(str::trim).find_map(|kv| kv.strip_prefix(&format!("{COOKIE}="))).map(str::to_string).filter(|t| !t.is_empty())
}

/// The signed-in admin, if the cookie is one.
pub(crate) async fn admin(app: &App, h: &HeaderMap) -> Option<(store::User, String)> {
    let token = cookie_token(h)?;
    let t = token.clone();
    let u = with_db(app, move |db| store::user_for_token(db, &t)).await.ok()?.ok()??;
    u.is_admin.then_some((u, token))
}

fn set_cookie(token: &str, secure: bool) -> String {
    format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000{}", if secure { "; Secure" } else { "" })
}

pub(crate) fn behind_https(h: &HeaderMap) -> bool {
    h.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).is_some_and(|v| v.eq_ignore_ascii_case("https"))
}

pub async fn home(State(app): State<App>, h: HeaderMap) -> Response {
    let users = with_db(&app, |db| store::user_count(db)).await.ok().and_then(|r| r.ok()).unwrap_or(0);
    if users == 0 {
        return setup_page("");
    }
    if admin(&app, &h).await.is_some() {
        return Redirect::to("/admin").into_response();
    }
    page(
        "Welcome",
        "<h1>BrowseCraft sync server</h1><div class=card><p>This server keeps BrowseCraft browsers in step: bookmarks, passwords, history, open tabs, extensions and settings.</p>\
<p>In BrowseCraft, go to <b>chrome://browsecraft</b> and sign in with this server's address and your user name.</p></div>\
<div class=card><h3>Admin</h3><form method=post action=/admin/sign-in><label>User name<input type=text name=user autocomplete=username></label>\
<label>Password<input type=password name=password autocomplete=current-password></label><button>Sign in</button></form></div>",
    )
}

fn setup_page(error: &str) -> Response {
    page(
        "Set up",
        &format!(
            "<h1>Set up BrowseCraft sync</h1><div class=card><p>Make the first user. They are the admin: they can add the others.</p>{}\
<form method=post action=/setup><label>User name<input type=text name=user autocomplete=username></label>\
<label>Password (at least 8 characters)<input type=password name=password autocomplete=new-password></label><button>Create</button></form></div>",
            if error.is_empty() { String::new() } else { format!("<p class=err>{}</p>", esc(error)) }
        ),
    )
}

#[derive(serde::Deserialize)]
pub struct Creds {
    user: String,
    password: String,
}

pub async fn setup(State(app): State<App>, h: HeaderMap, Form(f): Form<Creds>) -> Response {
    let r = with_db(&app, move |db| {
        // Only while there are no users: afterwards the admin page adds them.
        if store::user_count(db).map_err(|e| e.to_string())? != 0 {
            return Err("already set up".to_string());
        }
        store::add_user(db, &f.user, &f.password, true)?;
        store::sign_in(db, &f.user, &f.password, "Admin page").map(|(_, t)| t)
    })
    .await;
    match r {
        Ok(Ok(token)) => ([(header::SET_COOKIE, set_cookie(&token, behind_https(&h)))], Redirect::to("/admin")).into_response(),
        Ok(Err(e)) if e == "already set up" => Redirect::to("/").into_response(),
        Ok(Err(e)) => setup_page(&e),
        Err(r) => r,
    }
}

pub async fn sign_in(State(app): State<App>, h: HeaderMap, Form(f): Form<Creds>) -> Response {
    let r = crate::web::authenticate(&app, f.user, f.password, "Admin page".into()).await;
    match r {
        Ok(Ok((u, token))) if u.is_admin => ([(header::SET_COOKIE, set_cookie(&token, behind_https(&h)))], Redirect::to("/admin")).into_response(),
        Ok(Ok((_, token))) => {
            let _ = with_db(&app, move |db| store::sign_out(db, &token)).await;
            page("Not an admin", "<h1>Not an admin</h1><div class=card><p>That user can sync, but only admins can use these pages.</p><p><a href=/>Back</a></p></div>")
        }
        Ok(Err(e)) => page("Sign in", &format!("<h1>Sign in</h1><div class=card><p class=err>{}</p><p><a href=/>Try again</a></p></div>", esc(&e))),
        Err(r) => r,
    }
}

pub async fn sign_out(State(app): State<App>, h: HeaderMap) -> Response {
    if let Some(t) = cookie_token(&h) {
        let _ = with_db(&app, move |db| store::sign_out(db, &t)).await;
    }
    ([(header::SET_COOKIE, format!("{COOKIE}=; Path=/; Max-Age=0"))], Redirect::to("/")).into_response()
}

pub(crate) fn when(secs: Option<&str>) -> String {
    let Some(s) = secs.and_then(|s| s.parse::<u64>().ok()) else { return "never".into() };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let ago = now.saturating_sub(s);
    match ago {
        0..=119 => "just now".into(),
        120..=7199 => format!("{} minutes ago", ago / 60),
        7200..=172_799 => format!("{} hours ago", ago / 3600),
        _ => format!("{} days ago", ago / 86400),
    }
}

pub async fn admin_page(State(app): State<App>, h: HeaderMap) -> Response {
    admin_view(&app, &h, "").await
}

async fn admin_view(app: &App, h: &HeaderMap, note: &str) -> Response {
    let Some((me, _)) = admin(app, h).await else { return Redirect::to("/").into_response() };
    type Devices = Vec<(String, String, Option<String>)>;
    let data = with_db(app, |db| -> rusqlite::Result<Vec<(store::User, i64, String, Devices)>> {
        let mut out = vec![];
        for (u, _, n, source) in store::users(db)? {
            let devs = store::devices(db, u.id)?;
            out.push((u, n, source, devs));
        }
        Ok(out)
    })
    .await;
    let Ok(Ok(users)) = data else { return (StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response() };
    let mut rows = String::new();
    for (u, items, source, devs) in &users {
        let name = esc(&u.name);
        let devices: String = devs
            .iter()
            .map(|(d, created, seen)| format!("{} <span class=d>(signed in {}, last seen {})</span><br>", esc(d), when(Some(created)), when(seen.as_deref())))
            .collect();
        rows.push_str(&format!(
            "<tr><td><b>{name}</b>{}{}</td><td>{items}</td><td>{}</td><td>\
<form class=inline method=post action=/admin/password><input type=hidden name=user value=\"{name}\"><input type=password name=password placeholder=\"new password\" style=\"display:inline;width:10em\"> <button class=plain>Set</button></form><br>\
<form class=inline method=post action=/admin/sign-out-devices><input type=hidden name=user value=\"{name}\"><button class=plain style=\"margin-top:.4em\">Sign out all devices</button></form> \
{}</td></tr>",
            if u.is_admin { " <span class=d>(admin)</span>" } else { "" },
            if source == "domain" { "<br><span class=d>domain account</span>" } else { "" },
            if devices.is_empty() { "<span class=d>none</span>".into() } else { devices },
            if u.id == me.id {
                String::new()
            } else {
                format!("<form class=inline method=post action=/admin/delete><input type=hidden name=user value=\"{name}\"><button class=plain style=\"margin-top:.4em\">Remove</button></form>")
            }
        ));
    }
    page(
        "Admin",
        &format!(
            "<p style=float:right><a href=/admin/sign-out>Sign out</a></p><h1>BrowseCraft sync</h1>{}{domain}\
<div class=card><h3>Users</h3><table><tr><th>User</th><th>Items</th><th>Devices</th><th></th></tr>{rows}</table></div>\
<div class=card><h3>Add a user</h3><form method=post action=/admin/add><label>User name<input type=text name=user></label>\
<label>Password (at least 8 characters)<input type=password name=password autocomplete=new-password></label>\
<label style=font-weight:normal><input type=checkbox name=admin value=1> Admin (can use these pages)</label><button>Add</button></form></div>",
            if note.is_empty() { String::new() } else { format!("<p class=ok>{}</p>", esc(note)) },
            domain = match &app.ldap {
                Some(c) => format!(
                    "<div class=card><h3>Domain sign-in</h3><p>{}</p><p class=d>Anyone allowed in the domain can sign in with their domain user name and password; \
their account here is made the first time (or, if one of the same name exists, that one is used). The domain is asked first; a local password works when the domain says no or cannot be reached.</p></div>",
                    esc(&c.describe())
                ),
                None => String::new(),
            }
        ),
    )
}

#[derive(serde::Deserialize)]
pub struct AddUser {
    user: String,
    password: String,
    #[serde(default)]
    admin: Option<String>,
}

pub async fn add(State(app): State<App>, h: HeaderMap, Form(f): Form<AddUser>) -> Response {
    if admin(&app, &h).await.is_none() {
        return Redirect::to("/").into_response();
    }
    let name = f.user.clone();
    let r = with_db(&app, move |db| store::add_user(db, &f.user, &f.password, f.admin.is_some())).await;
    let note = match r {
        Ok(Ok(_)) => format!("Added {}.", name.trim()),
        Ok(Err(e)) => format!("Could not add {}: {e}", name.trim()),
        Err(r) => return r,
    };
    admin_view(&app, &h, &note).await
}

#[derive(serde::Deserialize)]
pub struct OneUser {
    user: String,
    #[serde(default)]
    password: String,
}

pub async fn password(State(app): State<App>, h: HeaderMap, Form(f): Form<OneUser>) -> Response {
    if admin(&app, &h).await.is_none() {
        return Redirect::to("/").into_response();
    }
    let name = f.user.clone();
    let note = match with_db(&app, move |db| store::set_password(db, &f.user, &f.password)).await {
        Ok(Ok(())) => format!("Password changed for {name}. Their browsers stay signed in."),
        Ok(Err(e)) => format!("Could not change it: {e}"),
        Err(r) => return r,
    };
    admin_view(&app, &h, &note).await
}

pub async fn sign_out_devices(State(app): State<App>, h: HeaderMap, Form(f): Form<OneUser>) -> Response {
    let Some((me, my_token)) = admin(&app, &h).await else { return Redirect::to("/").into_response() };
    let name = f.user.clone();
    let r = with_db(&app, move |db| store::sign_out_all(db, &f.user, (f.user.eq_ignore_ascii_case(&me.name)).then_some(my_token.as_str()))).await;
    let note = match r {
        Ok(Ok(n)) => format!("Signed out {n} devices for {name}."),
        Ok(Err(e)) => format!("Could not: {e}"),
        Err(r) => return r,
    };
    admin_view(&app, &h, &note).await
}

pub async fn delete(State(app): State<App>, h: HeaderMap, Form(f): Form<OneUser>) -> Response {
    let Some((me, _)) = admin(&app, &h).await else { return Redirect::to("/").into_response() };
    if f.user.trim().eq_ignore_ascii_case(&me.name) {
        return admin_view(&app, &h, "You can't remove yourself.").await;
    }
    let name = f.user.clone();
    let note = match with_db(&app, move |db| store::delete_user(db, &f.user)).await {
        Ok(Ok(1)) => format!("Removed {name} and everything synced for them."),
        Ok(Ok(_)) => format!("No user called {name}."),
        Ok(Err(e)) => format!("Could not: {e}"),
        Err(r) => return r,
    };
    admin_view(&app, &h, &note).await
}

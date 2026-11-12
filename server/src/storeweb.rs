//! The extension store over HTTP:
//!
//!   GET  /service/update2/crx    Chromium's older update check (gupdate XML), for extensions
//!                                whose update address is this server
//!   POST /service/update2/json   the update service Chromium uses for store extensions (protocol 4.0)
//!   GET  /ext/crx/<id>/<v>.crx   the packages themselves, byte for byte as the Chrome Web Store signed them
//!   GET  /ext/icon/<id>          an extension's icon
//!   GET  /webstore               the store page: what the signed-in person may install, and a
//!                                search of the Chrome Web Store (through this server) to ask for
//!                                more, or for admins to approve them there and then
//!   /admin/extensions...         approve, assign, hold or roll back (admins)
//!
//! BrowseCraft is pointed here by Group Policy (store address), so it never asks Google.

use std::collections::BTreeSet;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Form, Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::admin::{admin, behind_https, esc, page, when};
use crate::web::{App, with_db};
use crate::{crx, extstore, store};

const STORE_COOKIE: &str = "bc_store";

pub fn routes() -> Router<App> {
    Router::new()
        .route("/service/update2/crx", get(update_xml))
        .route("/service/update2/json", post(update_json))
        .route("/ext/crx/{id}/{file}", get(package))
        .route("/ext/icon/{id}", get(icon))
        .route("/webstore", get(store_page))
        .route("/webstore/", get(store_page))
        .route("/webstore/{*rest}", get(store_elsewhere))
        .route("/webstore/sign-in", post(store_sign_in))
        .route("/webstore/cws-icon", get(cws_icon))
        .route("/webstore/request", post(store_request))
        .route("/webstore/approve", post(store_approve))
        .route("/webstore/sign-out", get(store_sign_out))
        .route("/admin/extensions", get(admin_list))
        .route("/admin/extensions/add", post(admin_add))
        .route("/admin/extensions/check", post(admin_check))
        .route("/admin/extensions/use", post(admin_use))
        .route("/admin/extensions/auto", post(admin_auto))
        .route("/admin/extensions/assign", post(admin_assign))
        .route("/admin/extensions/unassign", post(admin_unassign))
        .route("/admin/extensions/remove", post(admin_remove))
        .route("/admin/extensions/dismiss", post(admin_dismiss))
}

/// Where browsers should fetch packages from: BROWSECRAFT_PUBLIC_URL, else what they asked for.
fn base_url(h: &HeaderMap) -> String {
    if let Ok(u) = std::env::var("BROWSECRAFT_PUBLIC_URL")
        && !u.trim().is_empty()
    {
        return u.trim_end_matches('/').to_string();
    }
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("localhost");
    format!("{}://{host}", if behind_https(h) { "https" } else { "http" })
}

fn package_url(base: &str, id: &str, version: &str) -> String {
    format!("{base}/ext/crx/{id}/{version}.crx")
}

/// Percent-decoding for query strings ("+" is a space).
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_pairs(q: &str) -> Vec<(String, String)> {
    q.split('&').filter(|p| !p.is_empty()).map(|p| {
        let (k, v) = p.split_once('=').unwrap_or((p, ""));
        (unescape(k), unescape(v))
    }).collect()
}

/// The older protocol: ?x=id%3D<id>%26v%3D<version>%26uc  (one x per extension).
async fn update_xml(State(app): State<App>, h: HeaderMap, RawQuery(q): RawQuery) -> Response {
    let base = base_url(&h);
    let asks: Vec<(String, String)> = query_pairs(q.as_deref().unwrap_or_default())
        .into_iter()
        .filter(|(k, _)| k == "x")
        .map(|(_, x)| {
            let inner = query_pairs(&x);
            let get = |k: &str| inner.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone()).unwrap_or_default();
            (get("id"), get("v"))
        })
        .collect();
    let redirect = query_pairs(q.as_deref().unwrap_or_default()).iter().any(|(k, v)| k == "response" && v == "redirect");
    eprintln!("update check (xml{}): {}", if redirect { ", redirect" } else { "" }, asks.iter().map(|(i, v)| format!("{i} {v}")).collect::<Vec<_>>().join(", "));
    let ids: Vec<String> = asks.iter().map(|(i, _)| i.clone()).collect();
    let found = with_db(&app, move |db| ids.iter().map(|id| extstore::current(db, id).ok().flatten()).collect::<Vec<_>>()).await;
    let Ok(found) = found else { return (StatusCode::INTERNAL_SERVER_ERROR, "server error").into_response() };
    // The store's download form (?response=redirect&x=id%3D..): straight to the package.
    if redirect {
        return match asks.first().zip(found.first()) {
            Some(((id, _), Some(c))) => Redirect::to(&package_url(&base, id, &c.version)).into_response(),
            _ => StatusCode::NOT_FOUND.into_response(),
        };
    }
    let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><gupdate xmlns="http://www.google.com/update2/response" protocol="2.0" server="browsecraft">"#);
    for ((id, have), cur) in asks.iter().zip(found) {
        let id_x = esc(id);
        match cur {
            None => xml.push_str(&format!(r#"<app appid="{id_x}" status="error-unknownApplication"/>"#)),
            Some(c) if !extstore::newer(&c.version, have) => xml.push_str(&format!(r#"<app appid="{id_x}" status="ok"><updatecheck status="noupdate"/></app>"#)),
            Some(c) => xml.push_str(&format!(
                r#"<app appid="{id_x}" status="ok"><updatecheck codebase="{}" hash_sha256="{}" size="{}" status="ok" version="{}"/></app>"#,
                esc(&package_url(&base, id, &c.version)),
                c.sha256,
                c.size,
                esc(&c.version)
            )),
        }
    }
    xml.push_str("</gupdate>");
    ([(header::CONTENT_TYPE, "text/xml; charset=utf-8")], xml).into_response()
}

/// The update service's protocol 4.0: {"request":{"apps":[{"appid","version",...}]}}.
async fn update_json(State(app): State<App>, h: HeaderMap, body: Bytes) -> Response {
    let base = base_url(&h);
    let Ok(req) = serde_json::from_slice::<Value>(&body) else { return (StatusCode::BAD_REQUEST, "not JSON").into_response() };
    let asks: Vec<(String, String)> = req["request"]["apps"]
        .as_array()
        .map(|a| a.iter().map(|x| (x["appid"].as_str().unwrap_or_default().to_string(), x["version"].as_str().unwrap_or("0").to_string())).collect())
        .unwrap_or_default();
    eprintln!("update check (json): {}", asks.iter().map(|(i, v)| format!("{i} {v}")).collect::<Vec<_>>().join(", "));
    let ids: Vec<String> = asks.iter().map(|(i, _)| i.clone()).collect();
    let Ok(found) = with_db(&app, move |db| ids.iter().map(|id| extstore::current(db, id).ok().flatten()).collect::<Vec<_>>()).await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "server error").into_response();
    };
    let apps: Vec<Value> = asks
        .iter()
        .zip(found)
        .map(|((id, have), cur)| match cur {
            None => json!({"appid": id, "status": "error-unknownApplication"}),
            Some(c) if !extstore::newer(&c.version, have) => json!({"appid": id, "status": "ok", "updatecheck": {"status": "noupdate"}}),
            Some(c) => json!({"appid": id, "status": "ok", "updatecheck": {
                "status": "ok",
                "nextversion": c.version,
                "pipelines": [{"pipeline_id": "browsecraft", "operations": [
                    {"type": "download", "size": c.size, "out": {"sha256": c.sha256}, "urls": [{"url": package_url(&base, id, &c.version)}]},
                    {"type": "crx3", "in": {"sha256": c.sha256}}
                ]}]
            }}),
        })
        .collect();
    let out = json!({"response": {"protocol": "4.0", "server": "browsecraft", "apps": apps}});
    ([(header::CONTENT_TYPE, "application/json; charset=utf-8")], format!(")]}}'\n{out}")).into_response()
}

async fn package(Path((id, file)): Path<(String, String)>) -> Response {
    let Some(version) = file.strip_suffix(".crx") else { return StatusCode::NOT_FOUND.into_response() };
    let Some(path) = extstore::crx_path(&id, version) else { return StatusCode::NOT_FOUND.into_response() };
    eprintln!("package download: {id} {version}");
    match tokio::fs::read(&path).await {
        Ok(data) => (
            [(header::CONTENT_TYPE, "application/x-chrome-extension"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")],
            data,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn icon(State(app): State<App>, Path(id): Path<String>) -> Response {
    let id2 = id.clone();
    let Ok(Ok(Some(cur))) = with_db(&app, move |db| extstore::current(db, &id2)).await else { return StatusCode::NOT_FOUND.into_response() };
    let Some(path) = extstore::crx_path(&id, &cur.version) else { return StatusCode::NOT_FOUND.into_response() };
    let Ok(data) = tokio::fs::read(&path).await else { return StatusCode::NOT_FOUND.into_response() };
    match crx::icon(&data, &extstore::manifest_json(&cur.manifest)) {
        Some((bytes, kind)) => ([(header::CONTENT_TYPE, kind), (header::CACHE_CONTROL, "public, max-age=86400")], bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

// ---------------------------------------------------------------------------------------------
// The store page.

fn store_cookie(h: &HeaderMap) -> Option<String> {
    let c = h.get(header::COOKIE)?.to_str().ok()?;
    c.split(';').map(str::trim).find_map(|kv| kv.strip_prefix(&format!("{STORE_COOKIE}="))).map(str::to_string).filter(|t| !t.is_empty())
}

async fn store_user(app: &App, h: &HeaderMap) -> Option<store::User> {
    let t = store_cookie(h)?;
    with_db(app, move |db| store::user_for_token(db, &t)).await.ok()?.ok()?
}

#[derive(Deserialize)]
struct Creds {
    user: String,
    password: String,
}

/// Addresses Chromium builds from the store's: "category/extensions" (chrome://extensions sidebar),
/// "detail/<id>" (an extension's store page): the store page, at that extension when there is one.
async fn store_elsewhere(Path(rest): Path<String>) -> Response {
    let id = rest.split('/').find(|p| crx::is_extension_id(p));
    match id {
        Some(id) => Redirect::to(&format!("/webstore#ext-{id}")).into_response(),
        None => Redirect::to("/webstore").into_response(),
    }
}

async fn store_sign_in(State(app): State<App>, h: HeaderMap, Form(f): Form<Creds>) -> Response {
    match crate::web::authenticate(&app, f.user, f.password, "Extension store".into()).await {
        Ok(Ok((_, token))) => (
            [(
                header::SET_COOKIE,
                format!("{STORE_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000{}", if behind_https(&h) { "; Secure" } else { "" }),
            )],
            Redirect::to("/webstore"),
        )
            .into_response(),
        Ok(Err(e)) => page("Extension store", &format!("<h1>Extension store</h1><div class=card><p class=err>{}</p><p><a href=/webstore>Try again</a></p></div>", esc(&e))),
        Err(r) => r,
    }
}

async fn store_sign_out(State(app): State<App>, h: HeaderMap) -> Response {
    if let Some(t) = store_cookie(&h) {
        let _ = with_db(&app, move |db| store::sign_out(db, &t)).await;
    }
    ([(header::SET_COOKIE, format!("{STORE_COOKIE}=; Path=/; Max-Age=0"))], Redirect::to("/webstore")).into_response()
}

const INSTALL_JS: &str = r#"<script>
async function install(btn){
  const id=btn.dataset.id, st=document.getElementById('s-'+id);
  const w=chrome&&chrome.webstorePrivate;
  if(!w){st.textContent='Open this page in BrowseCraft to install.';return}
  btn.disabled=true; st.textContent='Asking BrowseCraft...';
  // No iconUrl: Chromium fetches it as a public site with local-network access blocked, and
  // treats a data: URL as failed too, so a store on the LAN would get "Image decode failed".
  // The dialog shows a generic icon; the extension brings its own once installed.
  const details={id:id,manifest:btn.dataset.manifest,localizedName:btn.dataset.name};
  w.beginInstallWithManifest3(details,function(r){
    if(chrome.runtime.lastError||(r&&r!=='success'&&r!=='')){st.textContent='Not installed: '+((chrome.runtime.lastError&&chrome.runtime.lastError.message)||r);btn.disabled=false;return}
    st.textContent='Installing...';
    w.completeInstall(id,function(){
      if(chrome.runtime.lastError){st.textContent='Not installed: '+chrome.runtime.lastError.message;btn.disabled=false;return}
      st.textContent='Installed.';btn.textContent='Installed'});
  });
}
</script>"#;

/// For links back to a search.
fn url_enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// 8000000 -> "8,000,000".
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

async fn store_page(State(app): State<App>, h: HeaderMap, RawQuery(raw): RawQuery) -> Response {
    let pairs = query_pairs(raw.as_deref().unwrap_or(""));
    let arg = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
    let q: String = arg("q").chars().take(100).collect();
    let note = arg("note");
    let user = store_user(&app, &h).await;
    let groups: BTreeSet<String> = match &user {
        Some(u) => {
            let id = u.id;
            with_db(&app, move |db| store::groups(db, id)).await.ok().and_then(|r| r.ok()).unwrap_or_default().into_iter().collect()
        }
        None => BTreeSet::new(),
    };
    let who = extstore::Who { user: user.as_ref().map(|u| u.name.clone()).unwrap_or_default(), groups, ..Default::default() };
    type Row = (extstore::Item, Option<&'static str>, Option<extstore::Current>);
    let Ok(Ok(list)) = with_db(&app, move |db| -> rusqlite::Result<Vec<Row>> {
        let mut out = Vec::new();
        for it in extstore::items(db)? {
            let a = extstore::assignments(db, &it.id)?;
            let mode = extstore::mode_for(&a, &who);
            let cur = extstore::current(db, &it.id)?;
            out.push((it, mode, cur));
        }
        Ok(out)
    })
    .await
    else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "server error").into_response();
    };
    let mut body = String::from("<h1>Extension store</h1>");
    match &user {
        Some(u) => body.push_str(&format!(
            "<p class=d>Signed in as {} · <a href=/webstore/sign-out>Sign out</a></p>\
             <form method=get action=/webstore class=card style=\"display:flex;gap:.6em;align-items:end\">\
             <label style=\"flex:1;margin:0\">Find an extension<input type=search name=q value=\"{}\" placeholder=\"password manager, dark mode, grammar...\" autofocus></label>\
             <button>Search</button></form>{}",
            esc(&u.name),
            esc(&q),
            if note.is_empty() { String::new() } else { format!("<p class=ok>{}</p>", esc(&note)) }
        )),
        None => body.push_str(
            "<div class=card><form method=post action=/webstore/sign-in><p>Sign in to see the extensions your organisation offers you.</p>\
             <label>User<input type=text name=user autocomplete=username></label><label>Password<input type=password name=password autocomplete=current-password></label>\
             <button>Sign in</button></form></div>",
        ),
    }
    let needle = q.to_lowercase();
    let mut shown = 0;
    if !q.is_empty() && user.is_some() {
        body.push_str(&format!("<h2>Offered here, matching \u{201c}{}\u{201d}</h2>", esc(&q)));
    }
    for (it, mode, cur) in &list {
        let (Some(mode), Some(cur)) = (mode, cur) else { continue };
        if !needle.is_empty() && !it.name.to_lowercase().contains(&needle) && !it.description.to_lowercase().contains(&needle) {
            continue;
        }
        shown += 1;
        let manifest = esc(&cur.manifest);
        let action = if *mode == "install" {
            "<span class=d>Installed for you by your organisation</span>".to_string()
        } else {
            format!("<button data-id={id} data-name=\"{n}\" data-manifest=\"{manifest}\" onclick=install(this)>Add to BrowseCraft</button>", id = esc(&it.id), n = esc(&it.name))
        };
        body.push_str(&format!(
            "<div class=card id=ext-{id}><img src=/ext/icon/{id} alt=\"\" width=48 height=48 style=\"float:left;margin:0 1em 0 0\"><h2 style=margin:0>{n}</h2>\
             <p class=d style=margin:.2em\\ 0>{v}</p><p>{d}</p>{action} <span id=s-{id} class=d></span></div>",
            id = esc(&it.id),
            n = esc(&it.name),
            v = esc(&cur.version),
            d = esc(&it.description)
        ));
    }
    if shown == 0 && user.is_some() {
        body.push_str(if q.is_empty() { "<div class=card><p>Nothing here for you yet: search above to find one and ask for it.</p></div>" } else { "<div class=card><p class=d>None of the extensions offered here match.</p></div>" });
    }
    if let (Some(u), false) = (&user, q.is_empty()) {
        body.push_str(&webstore_results(&app, u, &q, &list).await);
    }
    body.push_str(INSTALL_JS);
    page("Extension store", &body)
}

/// The Chrome Web Store's results for `q`, searched by this server: ask for one, or (admins)
/// approve it here.
async fn webstore_results(app: &App, u: &store::User, q: &str, list: &[(extstore::Item, Option<&'static str>, Option<extstore::Current>)]) -> String {
    let query = q.to_string();
    let found = match tokio::task::spawn_blocking(move || extstore::search_webstore(&query)).await {
        Ok(Ok(f)) => f,
        Ok(Err(e)) => return format!("<h2>From the Chrome Web Store</h2><div class=card><p class=err>{}</p></div>", esc(&e)),
        Err(_) => return String::new(),
    };
    let uid = u.id;
    let asked = with_db(app, move |db| extstore::requested_by(db, uid)).await.ok().and_then(|r| r.ok()).unwrap_or_default();
    let mut b = String::from(
        "<h2>From the Chrome Web Store</h2><p class=d>Searched for you by this server. Nothing here is installed until an administrator approves it.</p>",
    );
    let mut n = 0;
    for f in found.iter().take(20) {
        let offered = list.iter().any(|(it, mode, cur)| it.id == f.id && mode.is_some() && cur.is_some());
        if offered {
            continue; // listed above
        }
        n += 1;
        let in_catalogue = list.iter().any(|(it, _, _)| it.id == f.id);
        let ret = format!("<input type=hidden name=id value={}><input type=hidden name=name value=\"{}\"><input type=hidden name=q value=\"{}\">", esc(&f.id), esc(&f.name), esc(q));
        let action = if u.is_admin && in_catalogue {
            format!("<span class=d>Approved, but not offered to you: <a href=/admin/extensions#ext-{}>choose who gets it</a></span>", esc(&f.id))
        } else if u.is_admin {
            format!("<form class=inline method=post action=/webstore/approve>{ret}<button>Approve for everyone</button></form>")
        } else if asked.contains(&f.id) {
            "<span class=d>Asked for: waiting for an administrator</span>".to_string()
        } else {
            format!("<form class=inline method=post action=/webstore/request>{ret}<button class=plain>Ask for it</button></form>")
        };
        let rating = if f.ratings > 0 { format!(" · \u{2605} {:.1} ({})", f.rating, thousands(f.ratings)) } else { String::new() };
        let host = f.site.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or("").trim_start_matches("www.");
        let site = if host.is_empty() { String::new() } else { format!(" · {}", esc(host)) };
        b.push_str(&format!(
            "<div class=card id=ext-{id}><img src=\"/webstore/cws-icon?u={icon}\" alt=\"\" width=48 height=48 style=\"float:left;margin:0 1em 0 0\"><h2 style=margin:0>{name}</h2>\
             <p class=d style=margin:.2em\\ 0>{users} users{rating}{site}</p><p>{summary}</p>{action}</div>",
            id = esc(&f.id),
            icon = url_enc(&f.icon),
            name = esc(&f.name),
            users = thousands(f.users),
            summary = esc(&f.summary),
        ));
    }
    if n == 0 {
        b.push_str("<div class=card><p class=d>Nothing more in the Chrome Web Store for that.</p></div>");
    }
    b
}

#[derive(Deserialize)]
struct UrlForm {
    u: String,
}

/// A Chrome Web Store icon, fetched by the server (signed-in store users only).
async fn cws_icon(State(app): State<App>, h: HeaderMap, axum::extract::Query(f): axum::extract::Query<UrlForm>) -> Response {
    if store_user(&app, &h).await.is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    match tokio::task::spawn_blocking(move || extstore::fetch_icon(&f.u)).await {
        Ok(Ok((ctype, data))) => ([(header::CONTENT_TYPE, ctype), (header::CACHE_CONTROL, "private, max-age=86400".to_string())], data).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
struct AskForm {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    q: String,
}

fn back_to(q: &str, note: &str) -> Response {
    Redirect::to(&format!("/webstore?q={}&note={}", url_enc(q), url_enc(note))).into_response()
}

async fn store_request(State(app): State<App>, h: HeaderMap, Form(f): Form<AskForm>) -> Response {
    let Some(u) = store_user(&app, &h).await else { return Redirect::to("/webstore").into_response() };
    let (id, name) = (f.id.clone(), f.name.clone());
    let note = match with_db(&app, move |db| extstore::request(db, &id, u.id, &name)).await {
        Ok(Ok(())) => {
            eprintln!("store: {} asked for {} ({})", u.name, f.name, f.id);
            format!("Asked for {}: an administrator will see it on the Extensions page.", f.name)
        }
        Ok(Err(e)) => e,
        Err(r) => return r,
    };
    back_to(&f.q, &note)
}

/// An admin, signed in to the store, approving an extension found in the search.
async fn store_approve(State(app): State<App>, h: HeaderMap, Form(f): Form<AskForm>) -> Response {
    let Some(u) = store_user(&app, &h).await else { return Redirect::to("/webstore").into_response() };
    if !u.is_admin {
        return back_to(&f.q, "Only administrators can approve extensions: ask for it instead.");
    }
    let note = approve_from_webstore(&app, f.id.clone()).await;
    eprintln!("store: {} approved {}: {note}", u.name, f.id);
    back_to(&f.q, &note)
}

/// Fetches an extension from the Chrome Web Store and approves it (offered to everyone); anyone
/// who asked for it no longer waits. The answer is the message to show.
async fn approve_from_webstore(app: &App, id: String) -> String {
    let fetched = tokio::task::spawn_blocking(move || extstore::fetch_from_webstore(&id)).await;
    match fetched {
        Ok(Ok((c, data))) => {
            let (name, version, id) = (c.name.clone(), c.version.clone(), c.id.clone());
            match with_db(app, move |db| {
                extstore::approve(db, &c, &data)?;
                let _ = extstore::clear_requests(db, &id);
                Ok::<(), String>(())
            })
            .await
            {
                Ok(Ok(())) => format!("Approved {name} {version}: offered to everyone. Change who gets it on the Extensions page."),
                Ok(Err(e)) => e,
                Err(_) => "server error".into(),
            }
        }
        Ok(Err(e)) => e,
        Err(_) => "server error".into(),
    }
}

// ---------------------------------------------------------------------------------------------
// Admin pages.

fn ext_id_from(s: &str) -> Option<String> {
    // An ID, or a Chrome Web Store link that ends with one.
    s.trim().trim_end_matches('/').split(['/', '?', '#']).find(|p| crx::is_extension_id(p)).map(str::to_string)
}

async fn admin_list(State(app): State<App>, h: HeaderMap) -> Response {
    admin_view(&app, &h, "").await
}

async fn admin_view(app: &App, h: &HeaderMap, note: &str) -> Response {
    if admin(app, h).await.is_none() {
        return Redirect::to("/").into_response();
    }
    type Row = (extstore::Item, Vec<extstore::Assignment>, Vec<(String, String)>);
    let Ok(Ok(rows)) = with_db(app, |db| -> rusqlite::Result<Vec<Row>> {
        extstore::items(db)?.into_iter().map(|it| Ok((extstore::assignments(db, &it.id)?, extstore::versions(db, &it.id)?, it))).map(|r| r.map(|(a, v, it)| (it, a, v))).collect()
    })
    .await
    else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "server error").into_response();
    };
    let requests = with_db(app, |db| extstore::requests(db)).await.ok().and_then(|r| r.ok()).unwrap_or_default();
    let mut b = format!(
        "<h1>Extensions</h1><p><a href=/admin>Users</a> · <a href=/webstore>Store page (search the Chrome Web Store)</a></p>{}\
         <div class=card><form method=post action=/admin/extensions/add><label>Add an extension: its Chrome Web Store link or ID\
         <input type=text name=id placeholder=\"https://chromewebstore.google.com/detail/.../ddkjiahejlhfcafbddmgiahcphecmpfh\"></label>\
         <button>Fetch and approve</button></form>\
         <form method=post action=/admin/extensions/check><button class=plain>Check the Chrome Web Store for updates now</button></form></div>",
        if note.is_empty() { String::new() } else { format!("<p class=ok>{}</p>", esc(note)) }
    );
    if !requests.is_empty() {
        b.push_str("<div class=card><h2 style=margin-top:0>Asked for</h2><table><tr><th>Extension</th><th>Who</th><th>Since</th><th></th></tr>");
        for r in &requests {
            let id = esc(&r.id);
            b.push_str(&format!(
                "<tr><td>{} <span class=d>{id}</span></td><td>{}</td><td>{}</td><td>\
                 <form class=inline method=post action=/admin/extensions/add><input type=hidden name=id value={id}><button>Approve</button></form> \
                 <form class=inline method=post action=/admin/extensions/dismiss><input type=hidden name=id value={id}><button class=plain>Dismiss</button></form></td></tr>",
                esc(&r.name),
                esc(&r.who.join(", ")),
                esc(&when(Some(&r.first))),
            ));
        }
        b.push_str("</table></div>");
    }
    for (it, assigns, versions) in &rows {
        let id = esc(&it.id);
        b.push_str(&format!(
            "<div class=card id=ext-{id}><img src=/ext/icon/{id} alt=\"\" width=40 height=40 style=\"float:right\"><h2 style=margin:0>{}</h2><p class=d>{id} · in use: <b>{}</b> · checked {}{}</p>",
            esc(&it.name),
            esc(&it.version),
            esc(&when(it.checked.as_deref())),
            it.check_error.as_deref().map(|e| format!(" · <span class=err>{}</span>", esc(e))).unwrap_or_default()
        ));
        b.push_str(&format!("<p class=d>Permissions: {}</p>", esc(&it.permissions.join(", "))));
        if let (Some(v), Some(why)) = (&it.held_version, &it.held_reason) {
            b.push_str(&format!(
                "<p class=err>Update {} is held: {}</p><form class=inline method=post action=/admin/extensions/use><input type=hidden name=id value={id}><input type=hidden name=version value=\"{}\"><button>Give everyone {}</button></form>",
                esc(v), esc(why), esc(v), esc(v)
            ));
        }
        b.push_str("<table><tr><th>Who</th><th>How</th><th></th></tr>");
        for a in assigns {
            let who = if a.scope == "everyone" { "Everyone".to_string() } else { format!("{}: {}", a.scope.replace('_', " "), esc(&a.value)) };
            b.push_str(&format!(
                "<tr><td>{who}</td><td>{}</td><td><form class=inline method=post action=/admin/extensions/unassign><input type=hidden name=id value={id}><input type=hidden name=scope value={}><input type=hidden name=value value=\"{}\"><button class=plain>Remove</button></form></td></tr>",
                if a.mode == "install" { "installed automatically" } else { "offered in the store" },
                esc(&a.scope),
                esc(&a.value)
            ));
        }
        b.push_str(&format!(
            "</table><form method=post action=/admin/extensions/assign><input type=hidden name=id value={id}>\
             <select name=scope><option value=everyone>Everyone</option><option value=user>User</option><option value=group>AD group</option><option value=device>PC</option><option value=device_group>AD computer group</option></select>\
             <input type=text name=value placeholder=\"name (not needed for Everyone)\" style=\"display:inline;width:14em\">\
             <select name=mode><option value=offer>offer in the store</option><option value=install>install automatically</option></select> <button class=plain>Add</button></form>"
        ));
        b.push_str(&format!(
            "<form class=inline method=post action=/admin/extensions/auto><input type=hidden name=id value={id}><input type=hidden name=auto value={}><button class=plain>{}</button></form> ",
            if it.auto_update { 0 } else { 1 },
            if it.auto_update { "Updates: automatic (click to hold each one for approval)" } else { "Updates: held for approval (click to make automatic)" }
        ));
        let older: Vec<&(String, String)> = versions.iter().filter(|(v, _)| *v != it.version && Some(v) != it.held_version.as_ref()).collect();
        for (v, _) in older {
            b.push_str(&format!(
                "<form class=inline method=post action=/admin/extensions/use><input type=hidden name=id value={id}><input type=hidden name=version value=\"{}\"><button class=plain>Roll back to {}</button></form> ",
                esc(v), esc(v)
            ));
        }
        b.push_str(&format!(
            "<form class=inline method=post action=/admin/extensions/remove onsubmit=\"return confirm('Remove {}? Browsers keep their copy but get no more updates from here.')\"><input type=hidden name=id value={id}><button class=plain>Remove</button></form></div>",
            esc(&it.name).replace('\'', "")
        ));
    }
    page("Extensions", &b)
}

#[derive(Deserialize)]
struct IdForm {
    id: String,
}

async fn admin_add(State(app): State<App>, h: HeaderMap, Form(f): Form<IdForm>) -> Response {
    if admin(&app, &h).await.is_none() {
        return Redirect::to("/").into_response();
    }
    let Some(id) = ext_id_from(&f.id) else { return admin_view(&app, &h, "That is not an extension ID or Chrome Web Store link.").await };
    let note = approve_from_webstore(&app, id).await;
    admin_view(&app, &h, &note).await
}

async fn admin_dismiss(State(app): State<App>, h: HeaderMap, Form(f): Form<IdForm>) -> Response {
    if admin(&app, &h).await.is_none() {
        return Redirect::to("/").into_response();
    }
    let id = f.id.clone();
    let _ = with_db(&app, move |db| extstore::clear_requests(db, &id)).await;
    admin_view(&app, &h, "Request dismissed.").await
}

async fn admin_check(State(app): State<App>, h: HeaderMap) -> Response {
    if admin(&app, &h).await.is_none() {
        return Redirect::to("/").into_response();
    }
    let note = check_all(&app).await;
    admin_view(&app, &h, &note).await
}

#[derive(Deserialize)]
struct UseForm {
    id: String,
    version: String,
}

async fn admin_use(State(app): State<App>, h: HeaderMap, Form(f): Form<UseForm>) -> Response {
    if admin(&app, &h).await.is_none() {
        return Redirect::to("/").into_response();
    }
    let (id, v) = (f.id.clone(), f.version.clone());
    let note = match with_db(&app, move |db| extstore::use_version(db, &id, &v)).await {
        Ok(Ok(())) => format!("Browsers now get {} {}.", f.id, f.version),
        Ok(Err(e)) => e,
        Err(r) => return r,
    };
    admin_view(&app, &h, &note).await
}

#[derive(Deserialize)]
struct AutoForm {
    id: String,
    auto: u8,
}

async fn admin_auto(State(app): State<App>, h: HeaderMap, Form(f): Form<AutoForm>) -> Response {
    if admin(&app, &h).await.is_none() {
        return Redirect::to("/").into_response();
    }
    let _ = with_db(&app, move |db| extstore::set_auto_update(db, &f.id, f.auto != 0)).await;
    Redirect::to("/admin/extensions").into_response()
}

#[derive(Deserialize)]
struct AssignForm {
    id: String,
    scope: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    mode: String,
}

async fn admin_assign(State(app): State<App>, h: HeaderMap, Form(f): Form<AssignForm>) -> Response {
    if admin(&app, &h).await.is_none() {
        return Redirect::to("/").into_response();
    }
    match with_db(&app, move |db| extstore::assign(db, &f.id, &f.scope, &f.value, &f.mode)).await {
        Ok(Ok(())) => Redirect::to("/admin/extensions").into_response(),
        Ok(Err(e)) => admin_view(&app, &h, &e).await,
        Err(r) => r,
    }
}

async fn admin_unassign(State(app): State<App>, h: HeaderMap, Form(f): Form<AssignForm>) -> Response {
    if admin(&app, &h).await.is_none() {
        return Redirect::to("/").into_response();
    }
    let _ = with_db(&app, move |db| extstore::unassign(db, &f.id, &f.scope, &f.value)).await;
    Redirect::to("/admin/extensions").into_response()
}

async fn admin_remove(State(app): State<App>, h: HeaderMap, Form(f): Form<IdForm>) -> Response {
    if admin(&app, &h).await.is_none() {
        return Redirect::to("/").into_response();
    }
    let _ = with_db(&app, move |db| extstore::remove(db, &f.id)).await;
    Redirect::to("/admin/extensions").into_response()
}

// ---------------------------------------------------------------------------------------------
// The update check: one question to the Chrome Web Store for every approved extension.

pub async fn check_all(app: &App) -> String {
    let Ok(Ok(items)) = with_db(app, |db| extstore::items(db)).await else { return "could not read the catalogue".into() };
    let have: Vec<(String, String, Option<String>)> = items.iter().map(|i| (i.id.clone(), i.version.clone(), i.held_version.clone())).collect();
    let ids: Vec<String> = have.iter().map(|(i, _, _)| i.clone()).collect();
    let offered = match tokio::task::spawn_blocking(move || extstore::webstore_versions(&ids)).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return e,
        Err(_) => return "server error".into(),
    };
    let mut notes = Vec::new();
    for (id, in_use, held) in have {
        let Some((_, v)) = offered.iter().find(|(a, _)| *a == id) else {
            let id2 = id.clone();
            let _ = with_db(app, move |db| extstore::note_check(db, &id2, Some("the Chrome Web Store no longer offers it"))).await;
            notes.push(format!("{id}: no longer in the Chrome Web Store"));
            continue;
        };
        if !extstore::newer(v, &in_use) || held.as_deref() == Some(v.as_str()) {
            let id2 = id.clone();
            let _ = with_db(app, move |db| extstore::note_check(db, &id2, None)).await;
            continue;
        }
        let id2 = id.clone();
        let fetched = tokio::task::spawn_blocking(move || extstore::fetch_from_webstore(&id2)).await;
        let outcome = match fetched {
            Ok(Ok((c, data))) => with_db(app, move |db| extstore::take_update(db, &c, &data)).await.unwrap_or(Err("server error".into())),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("server error".into()),
        };
        let id2 = id.clone();
        let err = outcome.as_ref().err().cloned();
        let _ = with_db(app, move |db| extstore::note_check(db, &id2, err.as_deref())).await;
        notes.push(match outcome {
            Ok(extstore::Outcome::Updated(v)) => format!("{id}: now {v}"),
            Ok(extstore::Outcome::Held(v, why)) => format!("{id}: {v} held ({why})"),
            Ok(_) => continue,
            Err(e) => format!("{id}: {e}"),
        });
    }
    if notes.is_empty() { "Everything is up to date.".into() } else { notes.join(" · ") }
}

/// Every BROWSECRAFT_EXT_CHECK_HOURS (default 4), check the Chrome Web Store for updates.
pub fn spawn_checker(app: App) {
    let hours: u64 = std::env::var("BROWSECRAFT_EXT_CHECK_HOURS").ok().and_then(|h| h.parse().ok()).filter(|h| *h > 0).unwrap_or(4);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(hours * 3600));
        tick.tick().await;
        loop {
            tick.tick().await;
            let note = check_all(&app).await;
            eprintln!("Extension update check: {note}");
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn query_strings_as_chromium_sends_them() {
        let q = "os=linux&x=id%3Dddkjiahejlhfcafbddmgiahcphecmpfh%26v%3D1.0%26uc&x=id%3Deimadpbcbfnmbkopoojfekhnkhdbieeh%26v%3D0.0.0.0";
        let xs: Vec<String> = query_pairs(q).into_iter().filter(|(k, _)| k == "x").map(|(_, v)| v).collect();
        assert_eq!(xs[0], "id=ddkjiahejlhfcafbddmgiahcphecmpfh&v=1.0&uc");
        assert_eq!(query_pairs(&xs[1])[1], ("v".to_string(), "0.0.0.0".to_string()));
    }

    #[test]
    fn ids_from_links() {
        assert_eq!(ext_id_from("https://chromewebstore.google.com/detail/dark-reader/eimadpbcbfnmbkopoojfekhnkhdbieeh?hl=en").as_deref(), Some("eimadpbcbfnmbkopoojfekhnkhdbieeh"));
        assert_eq!(ext_id_from(" ddkjiahejlhfcafbddmgiahcphecmpfh ").as_deref(), Some("ddkjiahejlhfcafbddmgiahcphecmpfh"));
        assert_eq!(ext_id_from("hello"), None);
    }
}

//! The HTTP side: the sync endpoint browsers talk to (Chromium posts to
//! "<sync url>/command/?client=...&client_id=..."), sign-in for browsers, and a status page.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use prost::Message;
use rusqlite::Connection;

use crate::{kerberos, ldap, proto, store, sync};

#[derive(Clone)]
pub struct App {
    pub(crate) db: Arc<Mutex<Connection>>,
    /// Domain sign-in, when it is set up.
    pub(crate) ldap: Option<Arc<ldap::Config>>,
}

#[cfg(test)]
pub fn router(db: Connection, ldap: Option<ldap::Config>) -> Router {
    router_for(App { db: Arc::new(Mutex::new(db)), ldap: ldap.map(Arc::new) })
}

fn router_for(app: App) -> Router {
    Router::new()
        .route("/", get(crate::admin::home))
        .route("/setup", post(crate::admin::setup))
        .route("/admin", get(crate::admin::admin_page))
        .route("/admin/sign-in", post(crate::admin::sign_in))
        .route("/admin/sign-out", get(crate::admin::sign_out))
        .route("/admin/add", post(crate::admin::add))
        .route("/admin/password", post(crate::admin::password))
        .route("/admin/sign-out-devices", post(crate::admin::sign_out_devices))
        .route("/admin/delete", post(crate::admin::delete))
        .route("/api/sign-in", post(sign_in))
        .route("/api/sign-in/negotiate", post(sign_in_negotiate))
        .route("/api/sign-out", post(sign_out))
        .route("/api/me", get(me))
        .route("/api/photo", get(get_photo).put(put_photo))
        .merge(crate::storeweb::routes())
        .fallback(command)
        .layer(DefaultBodyLimit::max(64 << 20))
        .with_state(app)
}

pub async fn serve(db: Connection, listen: &str, ldap: Option<ldap::Config>) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(listen).await.map_err(|e| format!("could not listen on {listen}: {e}"))?;
    eprintln!("BrowseCraft sync server on {listen}");
    match &ldap {
        Some(c) => eprintln!("Domain sign-in: {}", c.describe()),
        None => eprintln!("Domain sign-in: off (set BROWSECRAFT_LDAP_URL to use it)"),
    }
    match (kerberos::enabled(), ldap.as_ref().is_some_and(|c| c.can_look_up())) {
        (true, true) => eprintln!("Single sign-on: on (Kerberos)"),
        (true, false) => eprintln!("Single sign-on: off: a keytab is set but not BROWSECRAFT_LDAP_BIND_USER (and domain sign-in)"),
        (false, _) => eprintln!("Single sign-on: off (set KRB5_KTNAME to the server's keytab to use it)"),
    }
    let app = App { db: Arc::new(Mutex::new(db)), ldap: ldap.map(Arc::new) };
    crate::storeweb::spawn_checker(app.clone());
    axum::serve(listener, router_for(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| e.to_string())
}

/// Runs database work off the async threads.
#[allow(clippy::result_large_err)]
pub(crate) async fn with_db<T: Send + 'static>(app: &App, f: impl FnOnce(&mut Connection) -> T + Send + 'static) -> Result<T, Response> {
    let db = app.db.clone();
    tokio::task::spawn_blocking(move || {
        let mut g = db.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut g)
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "server error").into_response())
}

fn bearer(h: &HeaderMap) -> Option<String> {
    let v = h.get(header::AUTHORIZATION)?.to_str().ok()?;
    let t = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer "))?.trim();
    (!t.is_empty()).then(|| t.to_string())
}

#[allow(clippy::result_large_err)]
async fn signed_in(app: &App, h: &HeaderMap) -> Result<store::User, Response> {
    let Some(token) = bearer(h) else { return Err((StatusCode::UNAUTHORIZED, "sign in first").into_response()) };
    match with_db(app, move |db| store::user_for_token(db, &token)).await? {
        Ok(Some(u)) => Ok(u),
        Ok(None) => Err((StatusCode::UNAUTHORIZED, "that sign-in has ended; sign in again").into_response()),
        Err(_) => Err((StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response()),
    }
}

async fn command(State(app): State<App>, uri: Uri, h: HeaderMap, body: Bytes) -> Response {
    let path = uri.path().trim_end_matches('/');
    if !path.ends_with("/command") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let user = match signed_in(&app, &h).await {
        Ok(u) => u,
        Err(r) => return r,
    };
    // Chromium gzips its requests (components/sync/engine/net/http_bridge.cc).
    let gzipped = h.get(header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()).is_some_and(|v| v.eq_ignore_ascii_case("gzip"));
    let body = if gzipped {
        match gunzip(&body) {
            Some(b) => Bytes::from(b),
            None => {
                eprintln!("sync: {}: a gzip body that does not unpack", user.name);
                return (StatusCode::BAD_REQUEST, "bad gzip body").into_response();
            }
        }
    } else {
        body
    };
    let Ok(msg) = proto::ClientToServerMessage::decode(body) else {
        eprintln!("sync: {}: not a sync message", user.name);
        return (StatusCode::BAD_REQUEST, "not a sync message").into_response();
    };
    let r = with_db(&app, move |db| sync::handle(db, user.id, &msg)).await;
    match r {
        Ok(Ok(sync::Outcome::Ok(resp))) => ([(header::CONTENT_TYPE, "application/octet-stream")], resp.encode_to_vec()).into_response(),
        Ok(Ok(sync::Outcome::Bad(e))) => {
            eprintln!("sync: refused: {e}");
            (StatusCode::BAD_REQUEST, e).into_response()
        }
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("database error: {e}")).into_response(),
        Err(r) => r,
    }
}

/// Unpacks a gzip body, up to 64 MB.
fn gunzip(b: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(b).take(64 << 20).read_to_end(&mut out).ok()?;
    Some(out)
}

#[derive(serde::Deserialize)]
struct SignIn {
    user: String,
    password: String,
    #[serde(default)]
    device: String,
}

/// Signs a device in. With domain sign-in set up the domain is asked first (so a domain account
/// gets its domain admin and is linked even when its local password is the same); a local
/// password is the fallback when the domain says no such user or wrong password, or cannot be
/// reached. A domain account that is disabled, locked or not allowed is refused outright. The
/// error is the message to show.
#[allow(clippy::result_large_err)]
pub(crate) async fn authenticate(app: &App, user: String, password: String, device: String) -> Result<Result<(store::User, String), String>, Response> {
    let mut unreachable = false;
    if let Some(cfg) = app.ldap.clone() {
        match ldap::authenticate(&cfg, &user, &password).await {
            Ok(d) => return domain_sign_in(app, d, device).await,
            Err(ldap::Refused::WrongPassword) => {}
            Err(ldap::Refused::NotAllowed(why)) => return Ok(Err(why)),
            Err(ldap::Refused::Unavailable(why)) => {
                eprintln!("domain sign-in for {user}: {why}");
                unreachable = true;
            }
        }
    }
    let (u, p) = (user.clone(), password);
    match with_db(app, move |db| store::check_local(db, &u, &p)).await? {
        Err(e) => Ok(Err(e)),
        Ok(Some(account)) => {
            let id = account.id;
            Ok(with_db(app, move |db| store::issue_token(db, id, &device)).await?.map(|token| (account, token)))
        }
        Ok(None) if unreachable => Ok(Err("could not reach the domain controllers; try again soon".into())),
        Ok(None) => Ok(Err("wrong user name or password".into())),
    }
}

/// A domain user the domain has vouched for: their account here (made the first time), groups and
/// full name brought up to date, and a token for the device.
#[allow(clippy::result_large_err)]
async fn domain_sign_in(app: &App, d: ldap::DomainUser, device: String) -> Result<Result<(store::User, String), String>, Response> {
    let ldap::DomainUser { name, display_name, photo, is_admin, groups } = d;
    let account = match with_db(app, move |db| store::domain_user(db, &name, is_admin)).await? {
        Ok(account) => account,
        Err(e) => return Ok(Err(e)),
    };
    let uid = account.id;
    let _ = with_db(app, move |db| {
        let _ = store::set_groups(db, uid, &groups);
        if let Some(p) = &photo {
            let _ = store::set_photo(db, uid, p, "directory");
        }
        store::set_display_name(db, uid, &display_name)
    })
    .await;
    Ok(with_db(app, move |db| store::issue_token(db, uid, &device)).await?.map(|token| (account, token)))
}

/// What a browser gets back from signing in.
async fn signed_in_reply(app: &App, u: store::User, token: String) -> Response {
    let who = u.clone();
    let display_name = with_db(app, move |db| store::display_name(db, &who)).await.unwrap_or_else(|_| u.name.clone());
    axum::Json(serde_json::json!({ "token": token, "user": u, "displayName": display_name })).into_response()
}

#[derive(serde::Deserialize, Default)]
struct SignInNegotiate {
    #[serde(default)]
    device: String,
}

/// Single sign-on: the browser on a domain PC answers our "Negotiate" with a Kerberos ticket for
/// this server (nothing typed). With no ticket the answer is the 401 that asks for one.
async fn sign_in_negotiate(State(app): State<App>, h: HeaderMap, body: Bytes) -> Response {
    let refuse = |code: StatusCode, e: &str| (code, axum::Json(serde_json::json!({ "error": e }))).into_response();
    let Some(cfg) = app.ldap.clone().filter(|c| c.can_look_up() && kerberos::enabled()) else {
        return refuse(StatusCode::NOT_FOUND, "single sign-on is not set up on this server");
    };
    let ticket = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Negotiate ").or_else(|| v.strip_prefix("negotiate ")))
        .and_then(|t| base64::Engine::decode(&base64::engine::general_purpose::STANDARD, t.trim()).ok());
    let Some(ticket) = ticket else {
        return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Negotiate")], axum::Json(serde_json::json!({ "error": "no Kerberos ticket" })))
            .into_response();
    };
    let device = serde_json::from_slice::<SignInNegotiate>(&body).unwrap_or_default().device;
    let accepted = match tokio::task::spawn_blocking(move || kerberos::accept(&ticket)).await {
        Ok(Ok(a)) => a,
        Ok(Err(e)) => {
            eprintln!("single sign-on refused: {e}");
            // No new challenge: the browser would only send the same ticket again.
            return refuse(StatusCode::UNAUTHORIZED, &format!("the Kerberos ticket was not accepted: {e}"));
        }
        Err(_) => return refuse(StatusCode::INTERNAL_SERVER_ERROR, "server error"),
    };
    let Some(sam) = kerberos::account_in(&accepted.principal, cfg.domain()) else {
        eprintln!("single sign-on: {} is not a user of {}", accepted.principal, cfg.domain());
        return refuse(StatusCode::FORBIDDEN, &format!("{} is not a user of {}", accepted.principal, cfg.domain()));
    };
    let d = match ldap::look_up(&cfg, &sam).await {
        Ok(d) => d,
        Err(ldap::Refused::NotAllowed(why)) | Err(ldap::Refused::Unavailable(why)) => {
            eprintln!("single sign-on for {sam}: {why}");
            return refuse(StatusCode::FORBIDDEN, &why);
        }
        Err(ldap::Refused::WrongPassword) => return refuse(StatusCode::FORBIDDEN, "not allowed"),
    };
    let mut reply = match domain_sign_in(&app, d, device).await {
        Ok(Ok((u, token))) => {
            eprintln!("single sign-on: {} signed in", u.name);
            signed_in_reply(&app, u, token).await
        }
        Ok(Err(e)) => return refuse(StatusCode::FORBIDDEN, &e),
        Err(r) => return r,
    };
    // Mutual authentication: the browser can check it was really this server.
    if !accepted.reply.is_empty()
        && let Ok(v) = format!("Negotiate {}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &accepted.reply)).parse()
    {
        reply.headers_mut().insert(header::WWW_AUTHENTICATE, v);
    }
    reply
}

async fn sign_in(State(app): State<App>, axum::Json(s): axum::Json<SignIn>) -> Response {
    match authenticate(&app, s.user, s.password, s.device).await {
        Ok(Ok((u, token))) => signed_in_reply(&app, u, token).await,
        Ok(Err(e)) => (StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({ "error": e }))).into_response(),
        Err(r) => r,
    }
}

async fn sign_out(State(app): State<App>, h: HeaderMap) -> Response {
    let Some(token) = bearer(&h) else { return StatusCode::NO_CONTENT.into_response() };
    let _ = with_db(&app, move |db| store::sign_out(db, &token)).await;
    StatusCode::NO_CONTENT.into_response()
}

async fn me(State(app): State<App>, h: HeaderMap) -> Response {
    match signed_in(&app, &h).await {
        Ok(u) => {
            let who = u.clone();
            let (display_name, photo) =
                with_db(&app, move |db| (store::display_name(db, &who), store::photo_sha(db, who.id))).await.unwrap_or_else(|_| (u.name.clone(), None));
            axum::Json(serde_json::json!({ "user": u, "displayName": display_name, "photo": photo })).into_response()
        }
        Err(r) => r,
    }
}

/// The user's profile photo, for every browser they sign in to.
async fn get_photo(State(app): State<App>, h: HeaderMap) -> Response {
    let u = match signed_in(&app, &h).await {
        Ok(u) => u,
        Err(r) => return r,
    };
    match with_db(&app, move |db| store::photo(db, u.id)).await {
        Ok(Ok(Some((ctype, data, sha)))) => ([(header::CONTENT_TYPE, ctype), (header::ETAG, format!("\"{sha}\""))], data).into_response(),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "no photo").into_response(),
        Ok(Err(_)) => (StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
        Err(r) => r,
    }
}

/// A photo the user chose in one browser (PNG or JPEG, up to 2 MB): it replaces any other.
async fn put_photo(State(app): State<App>, h: HeaderMap, body: Bytes) -> Response {
    let u = match signed_in(&app, &h).await {
        Ok(u) => u,
        Err(r) => return r,
    };
    let name = u.name.clone();
    match with_db(&app, move |db| store::set_photo(db, u.id, &body, "user")).await {
        Ok(Ok(sha)) => {
            eprintln!("photo: {name} has a new profile photo");
            axum::Json(serde_json::json!({ "photo": sha })).into_response()
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, axum::Json(serde_json::json!({ "error": e }))).into_response(),
        Err(r) => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn call(app: &Router, req: axum::http::Request<axum::body::Body>) -> (StatusCode, Vec<u8>) {
        let r = app.clone().oneshot(req).await.unwrap();
        let s = r.status();
        (s, r.into_body().collect().await.unwrap().to_bytes().to_vec())
    }

    #[tokio::test]
    async fn photo_follows_the_user() {
        let db = store::open(":memory:").unwrap();
        let mark = store::add_user(&db, "mark", "a long password", true).unwrap();
        // A directory photo first, then the user's own; the directory never replaces theirs.
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
        assert!(store::set_photo(&db, mark.id, &jpeg, "directory").unwrap().is_some());
        let app = router(db, None);
        let sign_in = |device: &str| {
            axum::http::Request::post("/api/sign-in")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(r#"{{"user":"mark","password":"a long password","device":"{device}"}}"#)))
                .unwrap()
        };
        let token = |b: &[u8]| serde_json::from_slice::<serde_json::Value>(b).unwrap()["token"].as_str().unwrap().to_string();
        let (_, b) = call(&app, sign_in("Blue")).await;
        let blue = token(&b);
        let (_, b) = call(&app, sign_in("Purple")).await;
        let purple = token(&b);
        let png = b"\x89PNG\r\n\x1a\nnot really".to_vec();
        let put = |t: &str, body: Vec<u8>| axum::http::Request::put("/api/photo").header("authorization", format!("Bearer {t}")).body(axum::body::Body::from(body)).unwrap();
        assert_eq!(call(&app, put(&blue, b"GIF89a".to_vec())).await.0, StatusCode::BAD_REQUEST);
        assert_eq!(call(&app, put(&blue, png.clone())).await.0, StatusCode::OK);
        // Purple sees it, and /api/me names it.
        let get = |path: &str, t: &str| axum::http::Request::get(path).header("authorization", format!("Bearer {t}")).body(axum::body::Body::empty()).unwrap();
        let (s, b) = call(&app, get("/api/photo", &purple)).await;
        assert_eq!((s, b.to_vec()), (StatusCode::OK, png.clone()));
        let (_, b) = call(&app, get("/api/me", &purple)).await;
        let sha = serde_json::from_slice::<serde_json::Value>(&b).unwrap()["photo"].as_str().unwrap().to_string();
        assert_eq!(sha.len(), 64);
        assert_eq!(call(&app, get("/api/photo", "nobody")).await.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn sign_in_then_sync() {
        let db = store::open(":memory:").unwrap();
        store::add_user(&db, "mark", "a long password", true).unwrap();
        let app = router(db, None);
        let req = |body: &str| {
            axum::http::Request::post("/api/sign-in").header("content-type", "application/json").body(axum::body::Body::from(body.to_string())).unwrap()
        };
        let (s, _) = call(&app, req(r#"{"user":"mark","password":"wrong one!"}"#)).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, b) = call(&app, req(r#"{"user":"Mark","password":"a long password","device":"PC01"}"#)).await;
        assert_eq!(s, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        let token = v["token"].as_str().unwrap().to_string();
        // The sync endpoint, under any prefix, with the token.
        let msg = proto::ClientToServerMessage {
            message_contents: Some(proto::GET_UPDATES),
            get_updates: Some(proto::GetUpdatesMessage {
                from_progress_marker: vec![proto::DataTypeProgressMarker { data_type_id: Some(proto::BOOKMARKS), token: None, gc_directive: None }],
                need_encryption_key: None,
            }),
            ..Default::default()
        };
        let post = |auth: Option<&str>| {
            let mut r = axum::http::Request::post("/sync/command/?client=Chromium&client_id=abc");
            if let Some(a) = auth {
                r = r.header("authorization", format!("Bearer {a}"));
            }
            r.body(axum::body::Body::from(msg.encode_to_vec())).unwrap()
        };
        assert_eq!(call(&app, post(None)).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(call(&app, post(Some("not-a-token"))).await.0, StatusCode::UNAUTHORIZED);
        let (s, b) = call(&app, post(Some(&token))).await;
        assert_eq!(s, StatusCode::OK);
        // As Chromium sends it: gzipped.
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, &msg.encode_to_vec()).unwrap();
        let zipped = axum::http::Request::post("/sync/command/?client=Chromium")
            .header("authorization", format!("Bearer {token}"))
            .header("content-encoding", "gzip")
            .body(axum::body::Body::from(gz.finish().unwrap()))
            .unwrap();
        assert_eq!(call(&app, zipped).await.0, StatusCode::OK);
        let resp = proto::ClientToServerResponse::decode(b.as_slice()).unwrap();
        assert_eq!(resp.error_code, Some(proto::ERR_SUCCESS));
        assert!(resp.get_updates.unwrap().entries.len() >= 4);
        // Signed out: the token stops working.
        let so = axum::http::Request::post("/api/sign-out").header("authorization", format!("Bearer {token}")).body(axum::body::Body::empty()).unwrap();
        call(&app, so).await;
        assert_eq!(call(&app, post(Some(&token))).await.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn setup_then_admin() {
        let app = router(store::open(":memory:").unwrap(), None);
        let get = |path: &str, cookie: Option<&str>| {
            let mut r = axum::http::Request::get(path);
            if let Some(c) = cookie {
                r = r.header("cookie", c);
            }
            r.body(axum::body::Body::empty()).unwrap()
        };
        let form = |path: &str, body: &str, cookie: Option<&str>| {
            let mut r = axum::http::Request::post(path).header("content-type", "application/x-www-form-urlencoded");
            if let Some(c) = cookie {
                r = r.header("cookie", c);
            }
            r.body(axum::body::Body::from(body.to_string())).unwrap()
        };
        let (_, b) = call(&app, get("/", None)).await;
        assert!(String::from_utf8_lossy(&b).contains("Make the first user"));
        let r = app.clone().oneshot(form("/setup", "user=mark&password=a+long+password", None)).await.unwrap();
        let cookie = r.headers().get("set-cookie").unwrap().to_str().unwrap().split(';').next().unwrap().to_string();
        // Setup only works once.
        let r2 = app.clone().oneshot(form("/setup", "user=eve&password=another+password", None)).await.unwrap();
        assert!(r2.headers().get("set-cookie").is_none());
        let (_, b) = call(&app, get("/admin", Some(&cookie))).await;
        let html = String::from_utf8_lossy(&b).to_string();
        assert!(html.contains("<b>mark</b>") && !html.contains("eve"));
        let (_, b) = call(&app, form("/admin/add", "user=guest&password=guest+password", Some(&cookie))).await;
        assert!(String::from_utf8_lossy(&b).contains("Added guest."));
        // Not signed in: no admin page, no adding.
        let r = app.clone().oneshot(form("/admin/add", "user=x&password=xxxxxxxxxx", None)).await.unwrap();
        assert_eq!(r.status(), StatusCode::SEE_OTHER);
        let (_, b) = call(&app, form("/admin/delete", "user=guest", Some(&cookie))).await;
        assert!(String::from_utf8_lossy(&b).contains("Removed guest"));
        let (_, b) = call(&app, form("/admin/delete", "user=mark", Some(&cookie))).await;
        assert!(String::from_utf8_lossy(&b).contains("can&#39;t remove yourself"));
    }
}

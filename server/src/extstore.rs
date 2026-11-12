//! The extension store's catalogue: extensions an admin has approved, the packages the server
//! keeps for them (fetched from the Chrome Web Store, checked, kept byte for byte), who may have
//! them, and the update check against the Chrome Web Store.
//!
//! Packages live in BROWSECRAFT_EXT_DIR (default: an "extensions" folder beside the database) as
//! <id>/<version>.crx. The previous version stays so an admin can roll back.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::crx::{self, Crx};
use crate::store::now;

/// Chromium version we tell the Chrome Web Store we are (it serves packages that need no newer one).
pub const PRODVERSION: &str = "155.0.0.0";
const CWS_UPDATE: &str = "https://clients2.google.com/service/update2/crx";
const MAX_CRX: u64 = 256 << 20;

pub fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS ext_item (
             id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
             version TEXT NOT NULL, permissions TEXT NOT NULL DEFAULT '[]',
             auto_update INTEGER NOT NULL DEFAULT 1,
             held_version TEXT, held_reason TEXT,
             added TEXT NOT NULL, checked TEXT, check_error TEXT);
         CREATE TABLE IF NOT EXISTS ext_version (
             id TEXT NOT NULL, version TEXT NOT NULL, sha256 TEXT NOT NULL, size INTEGER NOT NULL,
             permissions TEXT NOT NULL DEFAULT '[]', manifest TEXT NOT NULL, fetched TEXT NOT NULL,
             PRIMARY KEY (id, version));
         -- scope: everyone | user | group | device | device_group;  mode: offer | install
         CREATE TABLE IF NOT EXISTS ext_assign (
             ext_id TEXT NOT NULL, scope TEXT NOT NULL, value TEXT NOT NULL DEFAULT '', mode TEXT NOT NULL,
             PRIMARY KEY (ext_id, scope, value));
         -- Extensions the server put into a user's synced extensions (so it removes only its own).
         CREATE TABLE IF NOT EXISTS ext_sync (
             user_id INTEGER NOT NULL, ext_id TEXT NOT NULL, PRIMARY KEY (user_id, ext_id));
         -- Extensions people have asked for (found by searching the Chrome Web Store).
         CREATE TABLE IF NOT EXISTS ext_request (
             ext_id TEXT NOT NULL, user_id INTEGER NOT NULL REFERENCES user(id) ON DELETE CASCADE,
             name TEXT NOT NULL, requested TEXT NOT NULL, PRIMARY KEY (ext_id, user_id));",
    )
}

pub fn dir() -> PathBuf {
    if let Ok(d) = std::env::var("BROWSECRAFT_EXT_DIR")
        && !d.trim().is_empty()
    {
        return PathBuf::from(d);
    }
    let db = std::env::var("BROWSECRAFT_DB").unwrap_or_else(|_| "browsecraft.db".into());
    Path::new(&db).parent().map(|p| p.join("extensions")).unwrap_or_else(|| PathBuf::from("extensions"))
}

pub fn crx_path(id: &str, version: &str) -> Option<PathBuf> {
    // Both come from URLs: only an ID's letters and a version's digits and dots get near the disk.
    if !crx::is_extension_id(id) || version.is_empty() || !version.bytes().all(|c| c.is_ascii_digit() || c == b'.') {
        return None;
    }
    Some(dir().join(id).join(format!("{version}.crx")))
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub permissions: Vec<String>,
    pub auto_update: bool,
    pub held_version: Option<String>,
    pub held_reason: Option<String>,
    pub checked: Option<String>,
    pub check_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Current {
    pub version: String,
    pub sha256: String,
    pub size: i64,
    pub manifest: String,
}

fn perms(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

pub fn items(db: &Connection) -> rusqlite::Result<Vec<Item>> {
    let mut st = db.prepare(
        "SELECT id, name, description, version, permissions, auto_update, held_version, held_reason, checked, check_error
         FROM ext_item ORDER BY name COLLATE NOCASE",
    )?;
    let rows = st.query_map([], |r| {
        Ok(Item {
            id: r.get(0)?,
            name: r.get(1)?,
            description: r.get(2)?,
            version: r.get(3)?,
            permissions: perms(&r.get::<_, String>(4)?),
            auto_update: r.get::<_, i64>(5)? != 0,
            held_version: r.get(6)?,
            held_reason: r.get(7)?,
            checked: r.get(8)?,
            check_error: r.get(9)?,
        })
    })?;
    rows.collect()
}

/// The version browsers are given for an approved extension.
pub fn current(db: &Connection, id: &str) -> rusqlite::Result<Option<Current>> {
    db.query_row(
        "SELECT v.version, v.sha256, v.size, v.manifest FROM ext_item i JOIN ext_version v ON v.id = i.id AND v.version = i.version
         WHERE i.id = ?1",
        [id],
        |r| Ok(Current { version: r.get(0)?, sha256: r.get(1)?, size: r.get(2)?, manifest: r.get(3)? }),
    )
    .optional()
}

/// Keep a checked package on disk and in the version table.
fn keep(db: &Connection, c: &Crx, data: &[u8]) -> Result<(), String> {
    let path = crx_path(&c.id, &c.version).ok_or("bad ID or version")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("could not make {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("crx.part");
    std::fs::write(&tmp, data).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("could not keep {}: {e}", path.display()))?;
    db.execute(
        "INSERT OR REPLACE INTO ext_version (id, version, sha256, size, permissions, manifest, fetched) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![c.id, c.version, c.sha256, c.size as i64, serde_json::to_string(&c.permissions).unwrap_or_default(), c.manifest.to_string(), now()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Approve an extension: the caller has fetched and checked its package already.
pub fn approve(db: &Connection, c: &Crx, data: &[u8]) -> Result<(), String> {
    if !c.from_webstore {
        return Err(format!("{} is not signed by the Chrome Web Store", c.id));
    }
    keep(db, c, data)?;
    db.execute(
        "INSERT INTO ext_item (id, name, description, version, permissions, added, checked) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, description = excluded.description, version = excluded.version,
           permissions = excluded.permissions, held_version = NULL, held_reason = NULL, checked = excluded.checked, check_error = NULL",
        params![c.id, c.name, c.description, c.version, serde_json::to_string(&c.permissions).unwrap_or_default(), now()],
    )
    .map_err(|e| e.to_string())?;
    // A new extension is offered to everyone until the admin says otherwise.
    db.execute("INSERT OR IGNORE INTO ext_assign (ext_id, scope, value, mode) SELECT ?1, 'everyone', '', 'offer' WHERE NOT EXISTS (SELECT 1 FROM ext_assign WHERE ext_id = ?1)", [&c.id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn remove(db: &Connection, id: &str) -> rusqlite::Result<()> {
    db.execute("DELETE FROM ext_item WHERE id = ?1", [id])?;
    db.execute("DELETE FROM ext_version WHERE id = ?1", [id])?;
    db.execute("DELETE FROM ext_assign WHERE ext_id = ?1", [id])?;
    if crx::is_extension_id(id) {
        let _ = std::fs::remove_dir_all(dir().join(id));
    }
    Ok(())
}

pub fn set_auto_update(db: &Connection, id: &str, auto: bool) -> rusqlite::Result<usize> {
    db.execute("UPDATE ext_item SET auto_update = ?2 WHERE id = ?1", params![id, auto as i64])
}

/// Give browsers a version the server already keeps: a held update the admin approves, or a roll back.
pub fn use_version(db: &Connection, id: &str, version: &str) -> Result<(), String> {
    let p: Option<String> = db
        .query_row("SELECT permissions FROM ext_version WHERE id = ?1 AND version = ?2", [id, version], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    let p = p.ok_or_else(|| format!("{id} {version} is not kept here"))?;
    db.execute(
        "UPDATE ext_item SET version = ?2, permissions = ?3, held_version = NULL, held_reason = NULL WHERE id = ?1",
        params![id, version, p],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn versions(db: &Connection, id: &str) -> rusqlite::Result<Vec<(String, String)>> {
    let mut st = db.prepare("SELECT version, fetched FROM ext_version WHERE id = ?1 ORDER BY fetched DESC")?;
    let rows = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

/// After a new version is kept, drop all but it, the version in use and the one before.
fn prune(db: &Connection, id: &str) -> rusqlite::Result<()> {
    let keep_n = 3;
    let in_use: Option<String> = db.query_row("SELECT version FROM ext_item WHERE id = ?1", [id], |r| r.get(0)).optional()?;
    let all = versions(db, id)?;
    for (v, _) in all.iter().skip(keep_n) {
        if Some(v) != in_use.as_ref() {
            db.execute("DELETE FROM ext_version WHERE id = ?1 AND version = ?2", [id, v])?;
            if let Some(p) = crx_path(id, v) {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    Ok(())
}

/// Dotted versions compared number by number ("1.10" > "1.9").
pub fn newer(a: &str, b: &str) -> bool {
    let p = |s: &str| s.split('.').map(|x| x.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (mut x, mut y) = (p(a), p(b));
    let n = x.len().max(y.len());
    x.resize(n, 0);
    y.resize(n, 0);
    x > y
}

// ---------------------------------------------------------------------------------------------
// Assignments: who may install, and who gets it installed automatically.

#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    pub scope: String,
    pub value: String,
    pub mode: String,
}

pub const SCOPES: [&str; 5] = ["everyone", "user", "group", "device", "device_group"];

pub fn assignments(db: &Connection, id: &str) -> rusqlite::Result<Vec<Assignment>> {
    let mut st = db.prepare("SELECT scope, value, mode FROM ext_assign WHERE ext_id = ?1 ORDER BY scope, value COLLATE NOCASE")?;
    let rows = st.query_map([id], |r| Ok(Assignment { scope: r.get(0)?, value: r.get(1)?, mode: r.get(2)? }))?;
    rows.collect()
}

pub fn assign(db: &Connection, id: &str, scope: &str, value: &str, mode: &str) -> Result<(), String> {
    if !SCOPES.contains(&scope) {
        return Err(format!("unknown scope {scope}"));
    }
    if mode != "offer" && mode != "install" {
        return Err(format!("unknown mode {mode}"));
    }
    let value = if scope == "everyone" { "" } else { value.trim() };
    if scope != "everyone" && value.is_empty() {
        return Err("say who: a user name, group, PC name or computer group".into());
    }
    db.execute("INSERT OR REPLACE INTO ext_assign (ext_id, scope, value, mode) VALUES (?1, ?2, ?3, ?4)", params![id, scope, value, mode])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn unassign(db: &Connection, id: &str, scope: &str, value: &str) -> rusqlite::Result<usize> {
    db.execute("DELETE FROM ext_assign WHERE ext_id = ?1 AND scope = ?2 AND value = ?3", params![id, scope, value])
}

/// Who is asking: the signed-in user, their groups, and the PC (when known).
#[derive(Debug, Default, Clone)]
pub struct Who {
    pub user: String,
    pub groups: BTreeSet<String>,
    pub device: String,
    pub device_groups: BTreeSet<String>,
}

/// The strongest mode an assignment list gives this person on this PC: "install" beats "offer".
pub fn mode_for(list: &[Assignment], who: &Who) -> Option<&'static str> {
    let mut best = None;
    for a in list {
        let hit = match a.scope.as_str() {
            "everyone" => true,
            "user" => !who.user.is_empty() && a.value.eq_ignore_ascii_case(&who.user),
            "group" => who.groups.iter().any(|g| g.eq_ignore_ascii_case(&a.value)),
            "device" => !who.device.is_empty() && a.value.eq_ignore_ascii_case(&who.device),
            "device_group" => who.device_groups.iter().any(|g| g.eq_ignore_ascii_case(&a.value)),
            _ => false,
        };
        if hit {
            if a.mode == "install" {
                return Some("install");
            }
            best = Some("offer");
        }
    }
    best
}

/// The extensions a sync user is to have installed automatically: assignments in "install" mode
/// for everyone, for them, or for one of their AD groups. (PC assignments are for the machine:
/// Group Policy's force-install list, not sync, which follows the person.) (id, version in use)
pub fn installs_for(db: &Connection, user_id: i64) -> rusqlite::Result<Vec<(String, String)>> {
    let name: String = db.query_row("SELECT name FROM user WHERE id = ?1", [user_id], |r| r.get(0)).optional()?.unwrap_or_default();
    let groups: BTreeSet<String> = {
        let mut st = db.prepare("SELECT name FROM user_group WHERE user_id = ?1")?;
        let rows = st.query_map([user_id], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let who = Who { user: name, groups, ..Default::default() };
    let mut out = Vec::new();
    for it in items(db)? {
        if it.held_version.as_deref() == Some(it.version.as_str()) {
            continue;
        }
        if mode_for(&assignments(db, &it.id)?, &who) == Some("install") {
            out.push((it.id, it.version));
        }
    }
    Ok(out)
}

pub fn synced_by_server(db: &Connection, user_id: i64) -> rusqlite::Result<Vec<String>> {
    let mut st = db.prepare("SELECT ext_id FROM ext_sync WHERE user_id = ?1")?;
    let rows = st.query_map([user_id], |r| r.get(0))?;
    rows.collect()
}

pub fn mark_synced(db: &Connection, user_id: i64, ext_id: &str, on: bool) -> rusqlite::Result<usize> {
    if on {
        db.execute("INSERT OR IGNORE INTO ext_sync (user_id, ext_id) VALUES (?1, ?2)", params![user_id, ext_id])
    } else {
        db.execute("DELETE FROM ext_sync WHERE user_id = ?1 AND ext_id = ?2", params![user_id, ext_id])
    }
}

// ---------------------------------------------------------------------------------------------
// Requests: extensions people found in the Chrome Web Store and asked for.

pub struct Request {
    pub id: String,
    pub name: String,
    /// Who asked (user names), and when the first did.
    pub who: Vec<String>,
    pub first: String,
}

pub fn request(db: &Connection, ext_id: &str, user_id: i64, name: &str) -> Result<(), String> {
    if !crx::is_extension_id(ext_id) {
        return Err("not an extension ID".into());
    }
    let name: String = name.chars().filter(|c| !c.is_control()).take(200).collect();
    db.execute(
        "INSERT OR IGNORE INTO ext_request (ext_id, user_id, name, requested) VALUES (?1, ?2, ?3, ?4)",
        params![ext_id, user_id, name, now()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// The extensions a user has asked for and not yet been given.
pub fn requested_by(db: &Connection, user_id: i64) -> rusqlite::Result<BTreeSet<String>> {
    let mut st = db.prepare("SELECT ext_id FROM ext_request WHERE user_id = ?1")?;
    let rows = st.query_map([user_id], |r| r.get(0))?;
    rows.collect()
}

pub fn requests(db: &Connection) -> rusqlite::Result<Vec<Request>> {
    let mut st = db.prepare(
        "SELECT r.ext_id, r.name, u.name, r.requested FROM ext_request r JOIN user u ON u.id = r.user_id ORDER BY r.requested, u.name",
    )?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))?;
    let mut out: Vec<Request> = Vec::new();
    for row in rows {
        let (id, name, who, when) = row?;
        match out.iter_mut().find(|r| r.id == id) {
            Some(r) => r.who.push(who),
            None => out.push(Request { id, name, who: vec![who], first: when }),
        }
    }
    Ok(out)
}

/// Approved or dismissed: nobody is waiting for it any more.
pub fn clear_requests(db: &Connection, ext_id: &str) -> rusqlite::Result<usize> {
    db.execute("DELETE FROM ext_request WHERE ext_id = ?1", [ext_id])
}

/// An extension found by searching the Chrome Web Store.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub summary: String,
    /// The developer's website as the Chrome Web Store lists it (helps tell copies apart).
    pub site: String,
    pub rating: f64,
    pub ratings: u64,
    pub users: u64,
}

/// Searches the Chrome Web Store (its search page, as a browser would see it) for extensions.
/// Blocking: call from spawn_blocking.
pub fn search_webstore(query: &str) -> Result<Vec<Found>, String> {
    let q: String = query.trim().chars().filter(|c| !c.is_control()).take(100).collect();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let mut enc = String::new();
    for b in q.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => enc.push(b as char),
            _ => enc.push_str(&format!("%{b:02X}")),
        }
    }
    let url = format!("https://chromewebstore.google.com/search/{enc}?hl=en-GB&itemTypes=EXTENSION");
    let body = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .get(&url)
        .set("User-Agent", &format!("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{PRODVERSION} Safari/537.36"))
        .set("Accept-Language", "en-GB,en;q=0.8")
        // Google's consent page would come first otherwise (this answers it: necessary cookies only).
        .set("Cookie", "SOCS=CAESEwgDEgk0ODE3Nzk3MjQaAmVuIAEaBgiA_LyaBg")
        .call()
        .map_err(|e| format!("the Chrome Web Store search failed: {e}"))?
        .into_string()
        .map_err(|e| e.to_string())?;
    Ok(parse_search(&body))
}

/// The results in a Chrome Web Store search page: its data is in AF_initDataCallback blocks,
/// each result an array [id, icon, name, rating, ratings, _, summary, ..., users at 14, ...].
pub fn parse_search(html: &str) -> Vec<Found> {
    fn walk(v: &Value, out: &mut Vec<Found>) {
        let Value::Array(a) = v else { return };
        if a.len() > 14
            && let (Some(id), Some(icon), Some(name)) = (a[0].as_str(), a[1].as_str(), a[2].as_str())
            && crx::is_extension_id(id)
            && icon.starts_with("https://")
        {
            if !out.iter().any(|f| f.id == id) {
                out.push(Found {
                    id: id.to_string(),
                    name: name.to_string(),
                    icon: icon.to_string(),
                    summary: a[6].as_str().unwrap_or_default().to_string(),
                    site: a[7].as_str().unwrap_or_default().to_string(),
                    rating: a[3].as_f64().unwrap_or(0.0),
                    ratings: a[4].as_u64().unwrap_or(0),
                    users: a[14].as_u64().unwrap_or(0),
                });
            }
            return;
        }
        for x in a {
            walk(x, out);
        }
    }
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("AF_initDataCallback(") {
        rest = &rest[i + 20..];
        let (Some(d), Some(end)) = (rest.find("data:"), rest.find(", sideChannel:")) else { break };
        if d < end
            && let Ok(v) = serde_json::from_str::<Value>(&rest[d + 5..end])
        {
            walk(&v, &mut out);
        }
        rest = &rest[end..];
    }
    out
}

/// A Chrome Web Store icon (lh3.googleusercontent.com only, 64 px), so the store page can show
/// it without the browser asking Google. Blocking.
pub fn fetch_icon(url: &str) -> Result<(String, Vec<u8>), String> {
    if !url.starts_with("https://lh3.googleusercontent.com/") || url.len() > 600 || url.contains(['?', '#', '=']) {
        return Err("not a Chrome Web Store icon".into());
    }
    let resp = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15)).build().get(&format!("{url}=s64")).call().map_err(|e| e.to_string())?;
    let ctype = resp.content_type().to_string();
    if !ctype.starts_with("image/") {
        return Err("not an image".into());
    }
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(resp.into_reader(), 1 << 20), &mut data).map_err(|e| e.to_string())?;
    Ok((ctype, data))
}

// ---------------------------------------------------------------------------------------------
// Talking to the Chrome Web Store (blocking: call from spawn_blocking).

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(120)).redirects(5).build()
}

/// Download an extension's current package from the Chrome Web Store and check it.
pub fn fetch_from_webstore(id: &str) -> Result<(Crx, Vec<u8>), String> {
    if !crx::is_extension_id(id) {
        return Err(format!("{id} is not an extension ID"));
    }
    let url = format!("{CWS_UPDATE}?response=redirect&prodversion={PRODVERSION}&acceptformat=crx3&x=id%3D{id}%26uc");
    let resp = agent().get(&url).call().map_err(|e| format!("the Chrome Web Store did not give {id}: {e}"))?;
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(resp.into_reader(), MAX_CRX), &mut data).map_err(|e| e.to_string())?;
    let c = crx::parse(&data)?;
    if c.id != id {
        return Err(format!("asked for {id} but the package is {}", c.id));
    }
    Ok((c, data))
}

/// One question to the Chrome Web Store for many extensions: the version it would give each.
pub fn webstore_versions(ids: &[String]) -> Result<Vec<(String, String)>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut url = format!("{CWS_UPDATE}?prodversion={PRODVERSION}&acceptformat=crx3");
    for id in ids.iter().filter(|i| crx::is_extension_id(i)) {
        url.push_str(&format!("&x=id%3D{id}%26v%3D0.0.0.0%26uc"));
    }
    let body = agent().get(&url).call().map_err(|e| format!("the Chrome Web Store update check failed: {e}"))?.into_string().map_err(|e| e.to_string())?;
    Ok(parse_gupdate(&body))
}

/// (appid, version) for every app in a gupdate XML answer that offers one.
pub fn parse_gupdate(xml: &str) -> Vec<(String, String)> {
    let attr = |tag: &str, name: &str| -> Option<String> {
        let key = format!("{name}=\"");
        let start = tag.find(&key)? + key.len();
        let end = tag[start..].find('"')? + start;
        Some(tag[start..end].to_string())
    };
    let mut out = Vec::new();
    for app in xml.split("<app ").skip(1) {
        let Some(appid) = attr(app, "appid") else { continue };
        let Some(uc) = app.split("<updatecheck").nth(1) else { continue };
        if attr(uc, "status").as_deref() != Some("ok") {
            continue;
        }
        if let Some(v) = attr(uc, "version") {
            out.push((appid, v));
        }
    }
    out
}

/// What an update check decided for one extension.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    UpToDate,
    Updated(String),
    Held(String, String),
}

/// Record a newly fetched version: in use straight away, or held for the admin when it asks for
/// more than the version in use, or when the extension is set to wait for approval.
pub fn take_update(db: &Connection, c: &Crx, data: &[u8]) -> Result<Outcome, String> {
    let item: Option<(String, String, i64)> = db
        .query_row("SELECT version, permissions, auto_update FROM ext_item WHERE id = ?1", [&c.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()
        .map_err(|e| e.to_string())?;
    let (in_use, old_perms, auto) = item.ok_or_else(|| format!("{} is not approved", c.id))?;
    if !newer(&c.version, &in_use) {
        return Ok(Outcome::UpToDate);
    }
    keep(db, c, data)?;
    let old: BTreeSet<String> = perms(&old_perms).into_iter().collect();
    let added: Vec<&String> = c.permissions.iter().filter(|p| !old.contains(*p)).collect();
    let outcome = if !added.is_empty() {
        let why = format!("asks for more: {}", added.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        db.execute("UPDATE ext_item SET held_version = ?2, held_reason = ?3 WHERE id = ?1", params![c.id, c.version, why]).map_err(|e| e.to_string())?;
        Outcome::Held(c.version.clone(), why)
    } else if auto == 0 {
        let why = "waiting for approval (updates are not automatic for this one)".to_string();
        db.execute("UPDATE ext_item SET held_version = ?2, held_reason = ?3 WHERE id = ?1", params![c.id, c.version, why]).map_err(|e| e.to_string())?;
        Outcome::Held(c.version.clone(), why)
    } else {
        use_version(db, &c.id, &c.version)?;
        Outcome::Updated(c.version.clone())
    };
    prune(db, &c.id).map_err(|e| e.to_string())?;
    Ok(outcome)
}

pub fn note_check(db: &Connection, id: &str, error: Option<&str>) -> rusqlite::Result<usize> {
    db.execute("UPDATE ext_item SET checked = ?2, check_error = ?3 WHERE id = ?1", params![id, now(), error])
}

/// Read the manifest JSON kept for a version (for the store page's install call).
pub fn manifest_json(m: &str) -> Value {
    serde_json::from_str(m).unwrap_or(Value::Null)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn webstore_search_results() {
        let found = parse_search(include_str!("testdata/cws-search.html"));
        assert!(found.len() >= 10, "{}", found.len());
        let bw = found.iter().find(|f| f.id == "nngceckbapebfimnlniiiahkandclblb").map(|f| (f.name.as_str(), f.users, f.ratings > 1000));
        assert_eq!(bw, Some(("Bitwarden Password Manager", 8000000, true)));
        assert_eq!(found.iter().find(|f| f.id == "nngceckbapebfimnlniiiahkandclblb").map(|f| f.site.as_str()), Some("https://bitwarden.com/"));
        assert!(found.iter().all(|f| f.icon.starts_with("https://lh3.googleusercontent.com/")));
    }

    #[test]
    fn requests_group_by_extension() {
        let db = crate::store::open(":memory:").expect("db");
        let a = crate::store::add_user(&db, "alice", "pw-alice-123", false).expect("alice");
        let b = crate::store::add_user(&db, "bob", "pw-bob-12345", false).expect("bob");
        let id = "nngceckbapebfimnlniiiahkandclblb";
        for u in [&a, &b, &a] {
            request(&db, id, u.id, "Bitwarden").expect("request");
        }
        let r = requests(&db).expect("requests");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].who, vec!["alice".to_string(), "bob".to_string()]);
        assert!(requested_by(&db, a.id).expect("mine").contains(id));
        assert_eq!(clear_requests(&db, id).expect("clear"), 2);
        assert!(request(&db, "not-an-id", a.id, "x").is_err());
    }

    fn db() -> Connection {
        let db = Connection::open_in_memory().expect("db");
        init(&db).expect("schema");
        db
    }

    fn fake(id: &str, version: &str, perms: &[&str]) -> Crx {
        Crx {
            id: id.into(),
            name: "Test".into(),
            version: version.into(),
            description: String::new(),
            permissions: perms.iter().map(|s| s.to_string()).collect(),
            manifest: serde_json::json!({"name": "Test", "version": version, "manifest_version": 3}),
            from_webstore: true,
            sha256: "00".into(),
            size: 4,
        }
    }

    #[test]
    fn updates_are_taken_or_held() {
        let tmp = std::env::temp_dir().join(format!("bc-ext-{}", std::process::id()));
        // SAFETY: tests in this module run one at a time on their own folder.
        unsafe { std::env::set_var("BROWSECRAFT_EXT_DIR", &tmp) };
        let db = db();
        let id = "eimadpbcbfnmbkopoojfekhnkhdbieeh";
        approve(&db, &fake(id, "4.9.1", &["storage"]), b"Cr24").expect("approve");
        assert_eq!(current(&db, id).expect("q").map(|c| c.version), Some("4.9.1".into()));
        assert_eq!(take_update(&db, &fake(id, "4.9.1", &["storage"]), b"Cr24").expect("same"), Outcome::UpToDate);
        assert_eq!(take_update(&db, &fake(id, "4.9.10", &["storage"]), b"Cr24").expect("newer"), Outcome::Updated("4.9.10".into()));
        // asks for a new permission: held, the old version stays in use
        let o = take_update(&db, &fake(id, "4.10.0", &["storage", "tabs"]), b"Cr24").expect("held");
        assert!(matches!(o, Outcome::Held(ref v, _) if v == "4.10.0"));
        assert_eq!(current(&db, id).expect("q").map(|c| c.version), Some("4.9.10".into()));
        use_version(&db, id, "4.10.0").expect("approve held");
        assert_eq!(current(&db, id).expect("q").map(|c| c.version), Some("4.10.0".into()));
        // roll back to a kept version
        use_version(&db, id, "4.9.10").expect("roll back");
        assert_eq!(current(&db, id).expect("q").map(|c| c.version), Some("4.9.10".into()));
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("1.10", "1.9"));
        assert!(newer("2026.1006.1931", "2026.926.1"));
        assert!(!newer("1.0", "1.0.0"));
    }

    #[test]
    fn who_gets_what() {
        let db = db();
        let id = "ddkjiahejlhfcafbddmgiahcphecmpfh";
        assign(&db, id, "group", "Sales", "offer").expect("a");
        assign(&db, id, "device", "RECEPTION-PC", "install").expect("b");
        let list = assignments(&db, id).expect("list");
        let mut who = Who { user: "bob".into(), ..Default::default() };
        assert_eq!(mode_for(&list, &who), None);
        who.groups.insert("sales".into());
        assert_eq!(mode_for(&list, &who), Some("offer"));
        who.device = "reception-pc".into();
        assert_eq!(mode_for(&list, &who), Some("install"));
        assert!(assign(&db, id, "user", "  ", "offer").is_err());
        assert!(assign(&db, id, "planet", "x", "offer").is_err());
    }

    #[test]
    fn reads_the_webstore_update_answer() {
        let xml = r#"<gupdate protocol="2.0"><app appid="aaaa" status="ok"><updatecheck codebase="https://x/a.crx" status="ok" version="1.2.3"/></app><app appid="bbbb" status="ok"><updatecheck status="noupdate"/></app></gupdate>"#;
        assert_eq!(parse_gupdate(xml), vec![("aaaa".to_string(), "1.2.3".to_string())]);
    }

    #[test]
    fn paths_only_from_ids_and_versions() {
        assert!(crx_path("ddkjiahejlhfcafbddmgiahcphecmpfh", "1.2.3").is_some());
        assert!(crx_path("../../etc", "1").is_none());
        assert!(crx_path("ddkjiahejlhfcafbddmgiahcphecmpfh", "1/../../x").is_none());
    }
}

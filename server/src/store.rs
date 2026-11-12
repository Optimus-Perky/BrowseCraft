//! The server's database (one SQLite file): users, their signed-in devices, and each user's
//! synced items.

use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

pub fn open(path: &str) -> rusqlite::Result<Connection> {
    let db = Connection::open(path)?;
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;
         CREATE TABLE IF NOT EXISTS user (
           id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE, pw_hash TEXT NOT NULL,
           is_admin INTEGER NOT NULL DEFAULT 0, created TEXT NOT NULL,
           store_birthday TEXT NOT NULL DEFAULT '', last_version INTEGER NOT NULL DEFAULT 0,
           keystore_key BLOB);
         CREATE TABLE IF NOT EXISTS device (
           token_hash BLOB PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES user(id) ON DELETE CASCADE,
           name TEXT NOT NULL, created TEXT NOT NULL, last_seen TEXT);
         CREATE TABLE IF NOT EXISTS entity (
           user_id INTEGER NOT NULL REFERENCES user(id) ON DELETE CASCADE, id TEXT NOT NULL,
           data_type INTEGER NOT NULL, version INTEGER NOT NULL, kind INTEGER NOT NULL,
           deleted INTEGER NOT NULL, parent_id TEXT NOT NULL DEFAULT '', body BLOB NOT NULL,
           PRIMARY KEY(user_id, id));
         CREATE INDEX IF NOT EXISTS entity_by_version ON entity(user_id, data_type, version);
         CREATE INDEX IF NOT EXISTS entity_by_parent ON entity(user_id, parent_id);",
    )?;
    // Where an account's sign-in comes from: 'local' (its own password) or 'domain'.
    let has_source: i64 = db.query_row("SELECT COUNT(*) FROM pragma_table_info('user') WHERE name = 'source'", [], |r| r.get(0))?;
    if has_source == 0 {
        db.execute_batch("ALTER TABLE user ADD COLUMN source TEXT NOT NULL DEFAULT 'local'")?;
    }
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS user_group (
           user_id INTEGER NOT NULL REFERENCES user(id) ON DELETE CASCADE, name TEXT NOT NULL COLLATE NOCASE,
           PRIMARY KEY(user_id, name));
         CREATE TABLE IF NOT EXISTS user_profile (
           user_id INTEGER PRIMARY KEY REFERENCES user(id) ON DELETE CASCADE, display_name TEXT NOT NULL);
         -- The profile photo every browser the user signs in to shows. source: user | directory.
         CREATE TABLE IF NOT EXISTS user_photo (
           user_id INTEGER PRIMARY KEY REFERENCES user(id) ON DELETE CASCADE, data BLOB NOT NULL,
           ctype TEXT NOT NULL, sha TEXT NOT NULL, source TEXT NOT NULL, updated TEXT NOT NULL);",
    )?;
    crate::extstore::init(&db)?;
    Ok(db)
}

/// A domain user's groups (nested included), as read at their last sign-in.
pub fn set_groups(db: &Connection, user_id: i64, groups: &[String]) -> rusqlite::Result<()> {
    db.execute("DELETE FROM user_group WHERE user_id = ?1", [user_id])?;
    for g in groups {
        db.execute("INSERT OR IGNORE INTO user_group (user_id, name) VALUES (?1, ?2)", rusqlite::params![user_id, g])?;
    }
    Ok(())
}

pub fn groups(db: &Connection, user_id: i64) -> rusqlite::Result<Vec<String>> {
    let mut st = db.prepare("SELECT name FROM user_group WHERE user_id = ?1 ORDER BY name")?;
    let rows = st.query_map([user_id], |r| r.get(0))?;
    rows.collect()
}

/// A domain user's full name (AD displayName), as read at their last sign-in.
pub fn set_display_name(db: &Connection, user_id: i64, name: &str) -> rusqlite::Result<()> {
    db.execute(
        "INSERT INTO user_profile (user_id, display_name) VALUES (?1, ?2)
         ON CONFLICT(user_id) DO UPDATE SET display_name = excluded.display_name",
        rusqlite::params![user_id, name],
    )?;
    Ok(())
}

/// The name to show for an account: its full name from the domain, else the user name.
pub fn display_name(db: &Connection, user: &User) -> String {
    db.query_row("SELECT display_name FROM user_profile WHERE user_id = ?1", [user.id], |r| r.get::<_, String>(0))
        .ok()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| user.name.clone())
}

/// What kind of image `data` is, for the photos (PNG and JPEG only).
pub fn image_type(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else {
        None
    }
}

/// Keeps a user's profile photo; returns its hash. A photo from the directory never replaces
/// one the user chose.
pub fn set_photo(db: &Connection, user_id: i64, data: &[u8], source: &str) -> Result<Option<String>, String> {
    let ctype = image_type(data).ok_or("not a PNG or JPEG image")?;
    if data.len() > 2 << 20 {
        return Err("that photo is over 2 MB".into());
    }
    if source == "directory" {
        let theirs: Option<String> =
            db.query_row("SELECT source FROM user_photo WHERE user_id = ?1", [user_id], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
        if theirs.as_deref() == Some("user") {
            return Ok(None);
        }
    }
    let sha: String = Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect();
    db.execute(
        "INSERT INTO user_photo (user_id, data, ctype, sha, source, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(user_id) DO UPDATE SET data = excluded.data, ctype = excluded.ctype, sha = excluded.sha,
           source = excluded.source, updated = excluded.updated",
        params![user_id, data, ctype, sha, source, now()],
    )
    .map_err(|e| e.to_string())?;
    Ok(Some(sha))
}

/// A user's photo: (content type, bytes, hash).
pub fn photo(db: &Connection, user_id: i64) -> rusqlite::Result<Option<(String, Vec<u8>, String)>> {
    db.query_row("SELECT ctype, data, sha FROM user_photo WHERE user_id = ?1", [user_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()
}

pub fn photo_sha(db: &Connection, user_id: i64) -> Option<String> {
    db.query_row("SELECT sha FROM user_photo WHERE user_id = ?1", [user_id], |r| r.get(0)).ok()
}

pub fn now() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    t.as_secs().to_string()
}

fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn hash_password(pw: &str) -> Result<String, String> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let mut salt = [0u8; 16];
    rand::rng().fill_bytes(&mut salt);
    let salt = SaltString::encode_b64(&salt).map_err(|e| e.to_string())?;
    argon2::Argon2::default().hash_password(pw.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| e.to_string())
}

fn check_password(pw: &str, hash: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    PasswordHash::new(hash).is_ok_and(|h| argon2::Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub is_admin: bool,
}

pub fn add_user(db: &Connection, name: &str, password: &str, admin: bool) -> Result<User, String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 100 || name.contains(char::is_whitespace) {
        return Err("a user name is one word, up to 100 characters".into());
    }
    if password.chars().count() < 8 {
        return Err("a password needs at least 8 characters".into());
    }
    let h = hash_password(password)?;
    db.execute("INSERT INTO user(name, pw_hash, is_admin, created) VALUES (?1, ?2, ?3, ?4)", params![name, h, admin as i64, now()])
        .map_err(|e| if e.to_string().contains("UNIQUE") { format!("there is already a user called {name}") } else { e.to_string() })?;
    Ok(User { id: db.last_insert_rowid(), name: name.to_string(), is_admin: admin })
}

pub fn set_password(db: &Connection, name: &str, password: &str) -> Result<(), String> {
    if password.chars().count() < 8 {
        return Err("a password needs at least 8 characters".into());
    }
    let h = hash_password(password)?;
    let n = db.execute("UPDATE user SET pw_hash = ?2 WHERE name = ?1", params![name.trim(), h]).map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("no user called {name}"));
    }
    Ok(())
}

/// Every user with their device count, synced item count and sign-in source ('local' / 'domain').
pub fn users(db: &Connection) -> rusqlite::Result<Vec<(User, i64, i64, String)>> {
    let mut q = db.prepare(
        "SELECT u.id, u.name, u.is_admin, (SELECT COUNT(*) FROM device d WHERE d.user_id = u.id),
                (SELECT COUNT(*) FROM entity e WHERE e.user_id = u.id AND e.deleted = 0 AND e.kind != 0), u.source
         FROM user u ORDER BY u.name",
    )?;
    q.query_map([], |r| Ok((User { id: r.get(0)?, name: r.get(1)?, is_admin: r.get::<_, i64>(2)? != 0 }, r.get(3)?, r.get(4)?, r.get(5)?)))?
        .collect()
}

pub fn user_count(db: &Connection) -> rusqlite::Result<i64> {
    db.query_row("SELECT COUNT(*) FROM user", [], |r| r.get(0))
}

pub fn delete_user(db: &Connection, name: &str) -> rusqlite::Result<usize> {
    db.execute("DELETE FROM user WHERE name = ?1", [name.trim()])
}

/// Signs a device in with a local password: a new token for it (shown once; only its hash is kept).
pub fn sign_in(db: &Connection, name: &str, password: &str, device: &str) -> Result<(User, String), String> {
    let user = check_local(db, name, password)?.ok_or("wrong user name or password")?;
    let token = issue_token(db, user.id, device)?;
    Ok((user, token))
}

/// The user, if `password` is their local password.
pub fn check_local(db: &Connection, name: &str, password: &str) -> Result<Option<User>, String> {
    let row: Option<(i64, String, bool, String)> = db
        .query_row("SELECT id, name, is_admin, pw_hash FROM user WHERE name = ?1", [name.trim()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0, r.get(3)?))
        })
        .optional()
        .map_err(|e| e.to_string())?;
    // The same work whether or not the user exists, so timing does not tell.
    let ok = match &row {
        Some((_, _, _, h)) => check_password(password, h),
        None => {
            static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
            let dummy = DUMMY.get_or_init(|| hash_password("not a real password").unwrap_or_default());
            let _ = check_password(password, dummy);
            false
        }
    };
    Ok(row.filter(|_| ok).map(|(id, name, is_admin, _)| User { id, name, is_admin }))
}

/// A new sign-in token for one of a user's devices.
pub fn issue_token(db: &Connection, user_id: i64, device: &str) -> Result<String, String> {
    let mut raw = [0u8; 32];
    rand::rng().fill_bytes(&mut raw);
    let token = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, raw);
    let device = device.trim().chars().take(100).collect::<String>();
    db.execute(
        "INSERT INTO device(token_hash, user_id, name, created) VALUES (?1, ?2, ?3, ?4)",
        params![hash_token(&token), user_id, if device.is_empty() { "Browser".to_string() } else { device }, now()],
    )
    .map_err(|e| e.to_string())?;
    Ok(token)
}

/// The account for a domain user who has just signed in: theirs from before, or one made now.
/// An existing account of the same name is used (and keeps everything synced to it). Admin comes
/// from the domain; an account that also has a local password keeps the admin it had.
pub fn domain_user(db: &Connection, name: &str, domain_admin: bool) -> Result<User, String> {
    let row: Option<(i64, String, bool, String)> = db
        .query_row("SELECT id, name, is_admin, pw_hash FROM user WHERE name = ?1", [name], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0, r.get(3)?))
        })
        .optional()
        .map_err(|e| e.to_string())?;
    match row {
        Some((id, name, was_admin, pw_hash)) => {
            let is_admin = domain_admin || (was_admin && !pw_hash.is_empty());
            db.execute("UPDATE user SET source = 'domain', is_admin = ?2 WHERE id = ?1", params![id, is_admin as i64]).map_err(|e| e.to_string())?;
            Ok(User { id, name, is_admin })
        }
        None => {
            db.execute(
                "INSERT INTO user(name, pw_hash, is_admin, created, source) VALUES (?1, '', ?2, ?3, 'domain')",
                params![name, domain_admin as i64, now()],
            )
            .map_err(|e| e.to_string())?;
            Ok(User { id: db.last_insert_rowid(), name: name.to_string(), is_admin: domain_admin })
        }
    }
}

/// Who a token belongs to (and notes the device was seen).
pub fn user_for_token(db: &Connection, token: &str) -> rusqlite::Result<Option<User>> {
    let h = hash_token(token);
    let u = db
        .query_row(
            "SELECT u.id, u.name, u.is_admin FROM device d JOIN user u ON u.id = d.user_id WHERE d.token_hash = ?1",
            [&h],
            |r| Ok(User { id: r.get(0)?, name: r.get(1)?, is_admin: r.get::<_, i64>(2)? != 0 }),
        )
        .optional()?;
    if u.is_some() {
        db.execute("UPDATE device SET last_seen = ?2 WHERE token_hash = ?1", params![h, now()])?;
    }
    Ok(u)
}

/// Signs out every device of a user (except, when given, the token in use).
pub fn sign_out_all(db: &Connection, name: &str, keep: Option<&str>) -> rusqlite::Result<usize> {
    let keep = keep.map(hash_token).unwrap_or_default();
    db.execute(
        "DELETE FROM device WHERE user_id = (SELECT id FROM user WHERE name = ?1) AND token_hash != ?2",
        params![name.trim(), keep],
    )
}

pub fn sign_out(db: &Connection, token: &str) -> rusqlite::Result<usize> {
    db.execute("DELETE FROM device WHERE token_hash = ?1", [hash_token(token)])
}

/// (device name, created, last seen) for a user.
pub fn devices(db: &Connection, user_id: i64) -> rusqlite::Result<Vec<(String, String, Option<String>)>> {
    let mut q = db.prepare("SELECT name, created, last_seen FROM device WHERE user_id = ?1 ORDER BY created")?;
    q.query_map([user_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::unwrap_used)]
    fn domain_accounts() {
        let db = open(":memory:").unwrap();
        // A new domain user gets an account with no local password (not even an empty one).
        let alice = domain_user(&db, "alice", false).unwrap();
        assert!(!alice.is_admin);
        assert_eq!(check_local(&db, "alice", "").unwrap(), None);
        assert_eq!(check_local(&db, "alice", "anything").unwrap(), None);
        // Admin follows the domain for domain-only accounts, both ways.
        assert!(domain_user(&db, "alice", true).unwrap().is_admin);
        assert!(!domain_user(&db, "alice", false).unwrap().is_admin);
        assert_eq!(domain_user(&db, "ALICE", false).unwrap().id, alice.id);

        // An existing local account of the same name is reused, keeps its password and its admin.
        let jo = add_user(&db, "jo.bloggs", "local-password", true).unwrap();
        let linked = domain_user(&db, "Jo.Bloggs", false).unwrap();
        assert_eq!(linked.id, jo.id);
        assert!(linked.is_admin);
        assert_eq!(check_local(&db, "jo.bloggs", "local-password").unwrap().map(|u| u.id), Some(jo.id));
        let sources: Vec<String> = users(&db).unwrap().into_iter().map(|(u, _, _, s)| format!("{}={s}", u.name)).collect();
        assert_eq!(sources, ["alice=domain", "jo.bloggs=domain"]);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn adds_the_source_column_to_an_old_database() {
        let path = std::env::temp_dir().join(format!("bc-old-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let old = Connection::open(&path).unwrap();
            old.execute_batch(
                "CREATE TABLE user (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE, pw_hash TEXT NOT NULL,
                 is_admin INTEGER NOT NULL DEFAULT 0, created TEXT NOT NULL, store_birthday TEXT NOT NULL DEFAULT '',
                 last_version INTEGER NOT NULL DEFAULT 0, keystore_key BLOB);
                 INSERT INTO user(name, pw_hash, created) VALUES ('old', 'x', '0');",
            )
            .unwrap();
        }
        let db = open(path.to_str().unwrap()).unwrap();
        assert_eq!(users(&db).unwrap()[0].3, "local");
        drop(db);
        let _ = std::fs::remove_file(&path);
    }
}

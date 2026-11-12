//! Chromium's sync protocol, served per user: a port of Chromium's LoopbackServer
//! (components/sync/engine/loopback_server, BSD licence, The Chromium Authors), the in-browser
//! server behind roaming profiles, onto SQLite with one store per user.
//!
//! Every item has a server-wide version (per user); a browser asks for each data type's items
//! newer than the version it last saw (its progress marker), and commits its own changes, which
//! get new versions. Bookmarks keep their folder tree; everything else is a flat set keyed by
//! the browser's client tag. What an item holds is opaque here (and, with a sync passphrase,
//! encrypted by the browser before it is sent).

use prost::Message;
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::proto::{self, ClientToServerMessage, ClientToServerResponse, SyncEntity};

/// How many items one GetUpdates returns at most (the browser asks again for the rest).
const MAX_BATCH: usize = 500;
/// How often browsers check for changes made elsewhere (there are no push notifications).
pub const POLL_SECONDS: i32 = 60;

const KIND_PERMANENT: i64 = 0;
const KIND_BOOKMARK: i64 = 1;
const KIND_UNIQUE: i64 = 2;
const KIND_TOMBSTONE: i64 = 3;

/// "{field number}_{inner id}", as LoopbackServerEntity::CreateId (no migrations here).
fn make_id(data_type: i32, inner: &str) -> String {
    format!("{data_type}_{inner}")
}

fn type_from_id(id: &str) -> Option<i32> {
    let (head, rest) = id.split_once('_')?;
    if rest.is_empty() {
        return None;
    }
    head.parse().ok()
}

fn root_tag(data_type: i32) -> Option<&'static str> {
    match data_type {
        proto::BOOKMARKS => Some("google_chrome_bookmarks"),
        proto::NIGORI => Some("google_chrome_nigori"),
        _ => None,
    }
}

/// A stored item: its kind, and the entity as the server sends it.
struct Stored {
    kind: i64,
    entity: SyncEntity,
}

struct Ctx<'a> {
    tx: &'a Transaction<'a>,
    user: i64,
    version: i64,
}

impl Ctx<'_> {
    fn get(&self, id: &str) -> rusqlite::Result<Option<Stored>> {
        self.tx
            .query_row("SELECT kind, body FROM entity WHERE user_id = ?1 AND id = ?2", params![self.user, id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
            })
            .optional()
            .map(|o| o.and_then(|(kind, body)| SyncEntity::decode(body.as_slice()).ok().map(|entity| Stored { kind, entity })))
    }

    /// Saves an item with the next version (SaveEntity).
    fn save(&mut self, kind: i64, data_type: i32, mut e: SyncEntity) -> rusqlite::Result<SyncEntity> {
        self.version += 1;
        e.version = Some(self.version);
        let id = e.id_string.clone().unwrap_or_default();
        self.tx.execute(
            "INSERT INTO entity(user_id, id, data_type, version, kind, deleted, parent_id, body) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(user_id, id) DO UPDATE SET data_type = excluded.data_type, version = excluded.version, kind = excluded.kind,
               deleted = excluded.deleted, parent_id = excluded.parent_id, body = excluded.body",
            params![
                self.user,
                id,
                data_type,
                self.version,
                kind,
                e.deleted.unwrap_or(false) as i64,
                e.parent_id_string.clone().unwrap_or_default(),
                e.encode_to_vec()
            ],
        )?;
        Ok(e)
    }

    /// A permanent folder (PersistentPermanentEntity).
    fn permanent(&mut self, data_type: i32, tag: &str, name: &str, parent: &str) -> rusqlite::Result<()> {
        let id = make_id(data_type, tag);
        if self.get(&id)?.is_some() {
            return Ok(());
        }
        let e = SyncEntity {
            id_string: Some(id),
            parent_id_string: Some(parent.to_string()),
            name: Some(name.to_string()),
            server_defined_unique_tag: Some(tag.to_string()),
            deleted: Some(false),
            folder: Some(true),
            specifics: Some(proto::default_specifics(data_type)),
            ..Default::default()
        };
        self.save(KIND_PERMANENT, data_type, e).map(|_| ())
    }

    /// A tombstone for an item (PersistentTombstoneEntity).
    fn tombstone(&mut self, id: &str, client_tag_hash: Option<String>) -> rusqlite::Result<Option<SyncEntity>> {
        let Some(data_type) = type_from_id(id) else { return Ok(None) };
        let e = SyncEntity {
            id_string: Some(id.to_string()),
            name: Some(String::new()),
            deleted: Some(true),
            folder: Some(false),
            specifics: Some(proto::default_specifics(data_type)),
            client_tag_hash: client_tag_hash.filter(|h| !h.is_empty()),
            ..Default::default()
        };
        self.save(KIND_TOMBSTONE, data_type, e).map(Some)
    }

    /// Tombstones everything under a deleted folder (DeleteChildren).
    fn delete_children(&mut self, parent: &str) -> rusqlite::Result<()> {
        let mut todo = vec![parent.to_string()];
        let mut seen = 0usize;
        while let Some(p) = todo.pop() {
            let kids: Vec<(String, Vec<u8>)> = self
                .tx
                .prepare("SELECT id, body FROM entity WHERE user_id = ?1 AND parent_id = ?2 AND deleted = 0")?
                .query_map(params![self.user, p], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            for (id, body) in kids {
                seen += 1;
                if seen > 1_000_000 {
                    return Ok(());
                }
                let hash = SyncEntity::decode(body.as_slice()).ok().and_then(|e| e.client_tag_hash);
                self.tombstone(&id, hash)?;
                todo.push(id);
            }
        }
        Ok(())
    }
}

/// The user's store: created on first use (birthday, keystore key, the Bookmarks and Nigori
/// roots). (birthday, last version, keystore key)
fn ensure_store(tx: &Transaction, user: i64) -> rusqlite::Result<(String, i64, Vec<u8>)> {
    let (birthday, version, key): (String, i64, Option<Vec<u8>>) =
        tx.query_row("SELECT store_birthday, last_version, keystore_key FROM user WHERE id = ?1", [user], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    if !birthday.is_empty()
        && let Some(k) = key
    {
        return Ok((birthday, version, k));
    }
    let birthday = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis().to_string();
    let mut k = vec![0u8; 16];
    rand::rng().fill_bytes(&mut k);
    let mut ctx = Ctx { tx, user, version };
    for t in [proto::BOOKMARKS, proto::NIGORI] {
        let name = if t == proto::BOOKMARKS { "Bookmarks" } else { "Nigori" };
        if let Some(tag) = root_tag(t) {
            ctx.permanent(t, tag, name, "0")?;
        }
    }
    tx.execute("UPDATE user SET store_birthday = ?2, keystore_key = ?3, last_version = ?4 WHERE id = ?1", params![user, birthday, k, ctx.version])?;
    Ok((birthday, ctx.version, k))
}

/// Starts the user's store again (ClearServerData): every item gone, a new birthday, so each
/// browser starts over.
fn clear(tx: &Transaction, user: i64) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM entity WHERE user_id = ?1", [user])?;
    tx.execute("UPDATE user SET store_birthday = '', keystore_key = NULL, last_version = 0 WHERE id = ?1", [user])?;
    Ok(())
}

pub enum Outcome {
    Ok(ClientToServerResponse),
    /// A request the protocol forbids (HTTP 400 / 500 in Chromium's server).
    Bad(String),
}

/// One request from a browser, for a signed-in user.
pub fn handle(db: &mut Connection, user: i64, msg: &ClientToServerMessage) -> rusqlite::Result<Outcome> {
    let tx = db.transaction()?;
    let (mut birthday, version, key) = ensure_store(&tx, user)?;
    let mut resp = ClientToServerResponse {
        client_command: Some(proto::ClientCommand { set_sync_poll_interval: Some(POLL_SECONDS) }),
        ..Default::default()
    };
    if let Some(b) = msg.store_birthday.as_deref().filter(|b| !b.is_empty())
        && b != birthday
    {
        resp.error_code = Some(proto::ERR_NOT_MY_BIRTHDAY);
        resp.store_birthday = Some(birthday);
        tx.commit()?;
        return Ok(Outcome::Ok(resp));
    }
    let mut ctx = Ctx { tx: &tx, user, version };
    match msg.message_contents.unwrap_or(0) {
        proto::GET_UPDATES => {
            let gu = msg.get_updates.clone().unwrap_or_default();
            if msg.store_birthday.as_deref().unwrap_or("").is_empty()
                && gu.from_progress_marker.iter().any(|m| !m.token.as_deref().unwrap_or_default().is_empty())
            {
                return Ok(Outcome::Bad("a progress marker without a store birthday".into()));
            }
            resp.get_updates = Some(get_updates(&mut ctx, &gu, &key)?);
        }
        proto::COMMIT => {
            let c = msg.commit.clone().unwrap_or_default();
            resp.commit = Some(commit(&mut ctx, &c)?);
        }
        proto::CLEAR_SERVER_DATA => {
            clear(&tx, user)?;
            let fresh = ensure_store(&tx, user)?;
            birthday = fresh.0;
            ctx.version = fresh.1;
            resp.clear_server_data = Some(proto::ClearServerDataResponse {});
        }
        other => return Ok(Outcome::Bad(format!("unknown request type {other}"))),
    }
    let last = ctx.version;
    tx.execute("UPDATE user SET last_version = ?2 WHERE id = ?1 AND last_version < ?2", params![user, last])?;
    resp.error_code = Some(proto::ERR_SUCCESS);
    resp.store_birthday = Some(birthday);
    tx.commit()?;
    Ok(Outcome::Ok(resp))
}

/// "123" or "migration/123": the version part.
fn token_version(token: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(token).ok()?;
    s.rsplit('/').next()?.parse().ok()
}

/// Google's update address: what Chrome Web Store extensions carry. BrowseCraft with the store
/// policy fetches them from the organisation's store; plain Chrome from the Chrome Web Store.
const WEBSTORE_UPDATE_URL: &str = "https://clients2.google.com/service/update2/crx";

/// The extension store's "install automatically" assignments, as synced extensions: added (or put
/// back after the person removed one), and taken away again when the assignment goes. Only items
/// the server added are ever removed.
fn assigned_extensions(ctx: &mut Ctx) -> rusqlite::Result<()> {
    let want = crate::extstore::installs_for(ctx.tx, ctx.user)?;
    let ours = crate::extstore::synced_by_server(ctx.tx, ctx.user)?;
    for (ext, version) in &want {
        let hash = proto::client_tag_hash(proto::EXTENSIONS, ext);
        let id = make_id(proto::EXTENSIONS, &hash);
        let present = ctx.get(&id)?.is_some_and(|s| s.kind != KIND_TOMBSTONE && !s.entity.deleted.unwrap_or(false));
        if present {
            continue;
        }
        let e = SyncEntity {
            id_string: Some(id),
            name: Some(ext.clone()),
            non_unique_name: Some(ext.clone()),
            deleted: Some(false),
            folder: Some(false),
            specifics: Some(proto::extension_specifics(ext, version, WEBSTORE_UPDATE_URL)),
            client_tag_hash: Some(hash),
            ..Default::default()
        };
        ctx.save(KIND_UNIQUE, proto::EXTENSIONS, e)?;
        crate::extstore::mark_synced(ctx.tx, ctx.user, ext, true)?;
    }
    for ext in ours.iter().filter(|o| !want.iter().any(|(w, _)| w == *o)) {
        let hash = proto::client_tag_hash(proto::EXTENSIONS, ext);
        let id = make_id(proto::EXTENSIONS, &hash);
        if ctx.get(&id)?.is_some_and(|s| s.kind != KIND_TOMBSTONE) {
            ctx.tombstone(&id, Some(hash))?;
        }
        crate::extstore::mark_synced(ctx.tx, ctx.user, ext, false)?;
    }
    Ok(())
}

fn get_updates(ctx: &mut Ctx, gu: &proto::GetUpdatesMessage, key: &[u8]) -> rusqlite::Result<proto::GetUpdatesResponse> {
    let mut out = proto::GetUpdatesResponse { changes_remaining: Some(0), ..Default::default() };
    if gu.from_progress_marker.iter().any(|m| m.data_type_id == Some(proto::EXTENSIONS)) {
        assigned_extensions(ctx)?;
    }
    // The bookmark folders appear on a browser's first bookmark sync.
    let first_bookmarks =
        gu.from_progress_marker.iter().any(|m| m.data_type_id == Some(proto::BOOKMARKS) && m.token.as_deref().unwrap_or_default().is_empty());
    if first_bookmarks {
        let root = make_id(proto::BOOKMARKS, "google_chrome_bookmarks");
        for (tag, name) in [("bookmark_bar", "Bookmark Bar"), ("other_bookmarks", "Other Bookmarks"), ("synced_bookmarks", "Synced Bookmarks")] {
            ctx.permanent(proto::BOOKMARKS, tag, name, &root)?;
        }
    }
    // What each type's browser has seen (None: first download, no tombstones then).
    let mut asked: Vec<(i32, Option<i64>)> = vec![];
    for m in &gu.from_progress_marker {
        let Some(t) = m.data_type_id else { continue };
        let tok = m.token.as_deref().unwrap_or_default();
        let seen = if tok.is_empty() { None } else { Some(token_version(tok).unwrap_or(0)) };
        if !asked.iter().any(|(a, _)| *a == t) {
            asked.push((t, seen));
        }
    }
    let mut wanted: Vec<(i64, i32, Vec<u8>)> = vec![];
    for (t, seen) in &asked {
        let rows: Vec<(i64, Vec<u8>)> = match seen {
            None => ctx
                .tx
                .prepare("SELECT version, body FROM entity WHERE user_id = ?1 AND data_type = ?2 AND deleted = 0 ORDER BY version")?
                .query_map(params![ctx.user, t], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?,
            Some(v) => ctx
                .tx
                .prepare("SELECT version, body FROM entity WHERE user_id = ?1 AND data_type = ?2 AND version > ?3 ORDER BY version")?
                .query_map(params![ctx.user, t, v], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?,
        };
        wanted.extend(rows.into_iter().map(|(v, b)| (v, *t, b)));
    }
    if wanted.len() > MAX_BATCH {
        wanted.sort_by_key(|w| w.0);
        out.changes_remaining = Some((wanted.len() - MAX_BATCH) as i64);
        wanted.truncate(MAX_BATCH);
    }
    let mut marks: Vec<(i32, i64)> = asked.iter().map(|(t, s)| (*t, s.unwrap_or(0))).collect();
    let mut keystore = gu.need_encryption_key.unwrap_or(false);
    for (v, t, body) in wanted {
        let Ok(e) = SyncEntity::decode(body.as_slice()) else { continue };
        if let Some(m) = marks.iter_mut().find(|m| m.0 == t) {
            m.1 = m.1.max(v);
        }
        if t == proto::NIGORI && proto::nigori_passphrase_type(e.specifics.as_deref().unwrap_or_default()) == proto::KEYSTORE_PASSPHRASE {
            keystore = true;
        }
        out.entries.push(e);
    }
    if keystore {
        out.encryption_keys.push(key.to_vec());
    }
    out.new_progress_marker = marks
        .into_iter()
        .map(|(t, v)| proto::DataTypeProgressMarker { data_type_id: Some(t), token: Some(v.to_string().into_bytes()), gc_directive: None })
        .collect();
    Ok(out)
}

fn is_lower_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_digit() || (b'a'..=b'f').contains(c),
        })
}

fn commit(ctx: &mut Ctx, c: &proto::CommitMessage) -> rusqlite::Result<proto::CommitResponse> {
    let mut out = proto::CommitResponse::default();
    let guid = c.cache_guid.clone().unwrap_or_default();
    // Items made in this commit get server ids; children sent in the same commit point at the
    // browser's temporary ids.
    let mut renamed: Vec<(String, String)> = vec![];
    for client in &c.entries {
        let parent = client.parent_id_string.clone().unwrap_or_default();
        let parent = renamed.iter().find(|(from, _)| *from == parent).map(|(_, to)| to.clone()).unwrap_or(parent);
        let r = commit_one(ctx, client, &guid, &parent)?;
        if let (Some(new_id), Some(old)) = (r.id_string.as_ref(), client.id_string.as_ref())
            && r.response_type == Some(proto::RESP_SUCCESS)
            && new_id != old
        {
            renamed.push((old.clone(), new_id.clone()));
        }
        out.entryresponse.push(r);
    }
    Ok(out)
}

fn invalid(msg: &str) -> proto::EntryResponse {
    proto::EntryResponse { response_type: Some(proto::RESP_INVALID_MESSAGE), error_message: Some(msg.to_string()), ..Default::default() }
}

fn commit_one(ctx: &mut Ctx, client: &SyncEntity, guid: &str, parent: &str) -> rusqlite::Result<proto::EntryResponse> {
    let client_id = client.id_string.clone().unwrap_or_default();
    let client_version = client.version.unwrap_or(0);
    let deleted = client.deleted.unwrap_or(false);
    if client_version == 0 && deleted {
        // Deleting what the server never had: nothing to do.
        return Ok(proto::EntryResponse { response_type: Some(proto::RESP_SUCCESS), id_string: Some(client_id), version: Some(0), ..Default::default() });
    }
    let Some(data_type) = proto::data_type_of(client.specifics.as_deref().unwrap_or_default()) else {
        return Ok(invalid("no data type"));
    };
    let server = ctx.get(&client_id)?;
    // A change made from an older copy than the server's: the browser fetches and merges.
    if let Some(s) = &server
        && client_version > 0
        && s.entity.version != Some(client_version)
    {
        return Ok(proto::EntryResponse { response_type: Some(proto::RESP_CONFLICT), id_string: Some(client_id), ..Default::default() });
    }
    let saved = if deleted {
        let Some(e) = ctx.tombstone(&client_id, client.client_tag_hash.clone())? else { return Ok(invalid("unknown item")) };
        ctx.delete_children(&client_id)?;
        e
    } else if data_type == proto::NIGORI {
        // Only the browser's own Nigori root, which the server made.
        let Some(s) = server else { return Ok(invalid("no nigori node")) };
        let mut e = s.entity;
        e.specifics = client.specifics.clone();
        ctx.save(KIND_PERMANENT, data_type, e)?
    } else if data_type == proto::BOOKMARKS {
        let (id, cache_guid, item_id, tag_hash) = match &server {
            Some(s) if s.kind != KIND_TOMBSTONE => (
                client_id.clone(),
                s.entity.originator_cache_guid.clone(),
                s.entity.originator_client_item_id.clone(),
                s.entity.client_tag_hash.clone(),
            ),
            Some(_) => {
                let inner = client_id.split_once('_').map(|(_, r)| r.to_string()).unwrap_or_default();
                (client_id.clone(), Some(guid.to_string()), Some(inner), client.client_tag_hash.clone())
            }
            None => {
                if client_version != 0 || !is_lower_uuid(&client_id) {
                    return Ok(invalid("a new bookmark's id must be its lowercase GUID"));
                }
                (make_id(proto::BOOKMARKS, &client_id), Some(guid.to_string()), Some(client_id.clone()), client.client_tag_hash.clone())
            }
        };
        let e = SyncEntity {
            id_string: Some(id),
            parent_id_string: Some(parent.to_string()),
            name: client.name.clone(),
            deleted: Some(false),
            folder: Some(client.folder.unwrap_or(false)),
            specifics: client.specifics.clone(),
            client_tag_hash: tag_hash.filter(|h| !h.is_empty()),
            originator_cache_guid: cache_guid,
            originator_client_item_id: item_id,
            ctime: client.ctime,
            mtime: client.mtime,
            unique_position: client.unique_position.clone(),
            ..Default::default()
        };
        ctx.save(KIND_BOOKMARK, data_type, e)?
    } else {
        let Some(hash) = client.client_tag_hash.clone().filter(|h| !h.is_empty()) else {
            return Ok(invalid("an item needs a client tag"));
        };
        let id = if client_version == 0 { make_id(data_type, &hash) } else { client_id.clone() };
        let e = SyncEntity {
            id_string: Some(id),
            name: client.name.clone(),
            deleted: Some(false),
            folder: Some(false),
            specifics: client.specifics.clone(),
            client_tag_hash: Some(hash),
            ctime: client.ctime,
            mtime: client.mtime,
            ..Default::default()
        };
        ctx.save(KIND_UNIQUE, data_type, e)?
    };
    Ok(proto::EntryResponse { response_type: Some(proto::RESP_SUCCESS), id_string: saved.id_string, version: saved.version, ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::*;

    fn db() -> (Connection, i64) {
        let db = crate::store::open(":memory:").unwrap();
        let u = crate::store::add_user(&db, "mark", "a long password", true).unwrap();
        (db, u.id)
    }

    fn ok(o: Outcome) -> ClientToServerResponse {
        match o {
            Outcome::Ok(r) => r,
            Outcome::Bad(e) => panic!("bad request: {e}"),
        }
    }

    fn get(db: &mut Connection, user: i64, birthday: &str, markers: &[(i32, &str)]) -> ClientToServerResponse {
        let msg = ClientToServerMessage {
            message_contents: Some(GET_UPDATES),
            store_birthday: Some(birthday.to_string()),
            get_updates: Some(GetUpdatesMessage {
                from_progress_marker: markers
                    .iter()
                    .map(|(t, tok)| DataTypeProgressMarker { data_type_id: Some(*t), token: Some(tok.as_bytes().to_vec()), gc_directive: None })
                    .collect(),
                need_encryption_key: Some(true),
            }),
            ..Default::default()
        };
        ok(handle(db, user, &msg).unwrap())
    }

    fn commit_msg(birthday: &str, entries: Vec<SyncEntity>) -> ClientToServerMessage {
        ClientToServerMessage {
            message_contents: Some(COMMIT),
            store_birthday: Some(birthday.to_string()),
            commit: Some(CommitMessage { entries, cache_guid: Some("cache-a".into()) }),
            ..Default::default()
        }
    }

    fn token(r: &ClientToServerResponse, t: i32) -> String {
        let gu = r.get_updates.as_ref().unwrap();
        let m = gu.new_progress_marker.iter().find(|m| m.data_type_id == Some(t)).unwrap();
        String::from_utf8(m.token.clone().unwrap()).unwrap()
    }

    #[test]
    fn first_sync_bookmarks_and_two_browsers() {
        let (mut db, u) = db();
        // Browser A: first download: the roots, the three folders, the keystore key.
        let r = get(&mut db, u, "", &[(BOOKMARKS, ""), (NIGORI, "")]);
        let bday = r.store_birthday.clone().unwrap();
        let gu = r.get_updates.clone().unwrap();
        let tags: Vec<String> = gu.entries.iter().filter_map(|e| e.server_defined_unique_tag.clone()).collect();
        for t in ["google_chrome_bookmarks", "google_chrome_nigori", "bookmark_bar", "other_bookmarks", "synced_bookmarks"] {
            assert!(tags.iter().any(|x| x == t), "{t} in {tags:?}");
        }
        assert_eq!(gu.encryption_keys.len(), 1);
        assert_eq!(r.client_command.as_ref().and_then(|c| c.set_sync_poll_interval), Some(POLL_SECONDS));
        let bm_token = token(&r, BOOKMARKS);
        // A folder with a bookmark inside, in one commit (the child points at the temporary id).
        let folder_id = "0f8e3a52-6d1c-4b8e-9b56-2a1f3c4d5e6f";
        let link_id = "1a2b3c4d-5e6f-4a1b-8c2d-3e4f5a6b7c8d";
        let bar = "32904_bookmark_bar";
        let r = ok(handle(
            &mut db,
            u,
            &commit_msg(
                &bday,
                vec![
                    SyncEntity {
                        id_string: Some(folder_id.into()),
                        parent_id_string: Some(bar.into()),
                        name: Some("Home lab".into()),
                        folder: Some(true),
                        version: Some(0),
                        specifics: Some(default_specifics(BOOKMARKS)),
                        ..Default::default()
                    },
                    SyncEntity {
                        id_string: Some(link_id.into()),
                        parent_id_string: Some(folder_id.into()),
                        name: Some("Proxmox".into()),
                        version: Some(0),
                        specifics: Some(default_specifics(BOOKMARKS)),
                        ..Default::default()
                    },
                ],
            ),
        )
        .unwrap());
        let er = r.commit.unwrap().entryresponse;
        assert!(er.iter().all(|e| e.response_type == Some(RESP_SUCCESS)), "{er:?}");
        let server_folder = er[0].id_string.clone().unwrap();
        assert_eq!(server_folder, format!("32904_{folder_id}"));
        // Browser B (already synced to bm_token) gets both, the child under the server's folder id.
        let r = get(&mut db, u, &bday, &[(BOOKMARKS, &bm_token)]);
        let gu = r.get_updates.clone().unwrap();
        assert_eq!(gu.entries.len(), 2);
        let link = gu.entries.iter().find(|e| e.name.as_deref() == Some("Proxmox")).unwrap();
        assert_eq!(link.parent_id_string.as_deref(), Some(server_folder.as_str()));
        assert_eq!(link.originator_client_item_id.as_deref(), Some(link_id));
        // Nothing more after that.
        let t2 = token(&r, BOOKMARKS);
        let r = get(&mut db, u, &bday, &[(BOOKMARKS, &t2)]);
        assert!(r.get_updates.unwrap().entries.is_empty());
        // A stale edit is a conflict; the right version goes through.
        let folder_v = er[0].version.unwrap();
        let mut edit = SyncEntity {
            id_string: Some(server_folder.clone()),
            parent_id_string: Some(bar.into()),
            name: Some("Lab".into()),
            folder: Some(true),
            version: Some(folder_v - 1),
            specifics: Some(default_specifics(BOOKMARKS)),
            ..Default::default()
        };
        let r = ok(handle(&mut db, u, &commit_msg(&bday, vec![edit.clone()])).unwrap());
        assert_eq!(r.commit.unwrap().entryresponse[0].response_type, Some(RESP_CONFLICT));
        edit.version = Some(folder_v);
        let r = ok(handle(&mut db, u, &commit_msg(&bday, vec![edit.clone()])).unwrap());
        let ev = r.commit.unwrap().entryresponse[0].version.unwrap();
        // Deleting the folder tombstones what is inside.
        edit.version = Some(ev);
        edit.deleted = Some(true);
        let r = ok(handle(&mut db, u, &commit_msg(&bday, vec![edit])).unwrap());
        assert_eq!(r.commit.unwrap().entryresponse[0].response_type, Some(RESP_SUCCESS));
        let r = get(&mut db, u, &bday, &[(BOOKMARKS, &t2)]);
        let gone: Vec<bool> = r.get_updates.unwrap().entries.iter().map(|e| e.deleted.unwrap_or(false)).collect();
        assert_eq!(gone, vec![true, true], "folder and child both tombstoned");
        // A new browser's first download leaves tombstones out.
        let r = get(&mut db, u, &bday, &[(BOOKMARKS, "")]);
        assert!(r.get_updates.unwrap().entries.iter().all(|e| !e.deleted.unwrap_or(false)));
    }

    #[test]
    fn passwords_birthday_and_clear() {
        let (mut db, u) = db();
        let bday = get(&mut db, u, "", &[(45873, "")]).store_birthday.unwrap();
        let pw = SyncEntity {
            id_string: Some("c1".into()),
            version: Some(0),
            client_tag_hash: Some("hashA".into()),
            specifics: Some(default_specifics(45873)),
            ..Default::default()
        };
        let r = ok(handle(&mut db, u, &commit_msg(&bday, vec![pw.clone()])).unwrap());
        assert_eq!(r.commit.unwrap().entryresponse[0].id_string.as_deref(), Some("45873_hashA"));
        let no_tag = SyncEntity { client_tag_hash: None, ..pw };
        let r = ok(handle(&mut db, u, &commit_msg(&bday, vec![no_tag])).unwrap());
        assert_eq!(r.commit.unwrap().entryresponse[0].response_type, Some(RESP_INVALID_MESSAGE));
        // Wrong birthday: start again.
        let r = get(&mut db, u, "12345", &[(45873, "3")]);
        assert_eq!(r.error_code, Some(ERR_NOT_MY_BIRTHDAY));
        // Clearing makes a new birthday and empties the store.
        let clear = ClientToServerMessage { message_contents: Some(CLEAR_SERVER_DATA), store_birthday: Some(bday.clone()), ..Default::default() };
        std::thread::sleep(std::time::Duration::from_millis(5));
        let r = ok(handle(&mut db, u, &clear).unwrap());
        assert_ne!(r.store_birthday.as_deref(), Some(bday.as_str()));
        let nb = r.store_birthday.unwrap();
        assert!(get(&mut db, u, &nb, &[(45873, "")]).get_updates.unwrap().entries.is_empty());
        // A marker without a birthday is refused.
        let msg = ClientToServerMessage {
            message_contents: Some(GET_UPDATES),
            get_updates: Some(GetUpdatesMessage {
                from_progress_marker: vec![DataTypeProgressMarker { data_type_id: Some(45873), token: Some(b"5".to_vec()), gc_directive: None }],
                need_encryption_key: None,
            }),
            ..Default::default()
        };
        assert!(matches!(handle(&mut db, u, &msg).unwrap(), Outcome::Bad(_)));
    }

    #[test]
    fn store_assignments_arrive_as_synced_extensions() {
        let (mut db, user) = db();
        let dark = "eimadpbcbfnmbkopoojfekhnkhdbieeh";
        db.execute(
            "INSERT INTO ext_item (id, name, version, added) VALUES (?1, 'Dark Reader', '4.9.133', '0')",
            [dark],
        )
        .unwrap();
        crate::extstore::assign(&db, dark, "everyone", "", "install").unwrap();
        // First extensions download: the assigned extension is there, with Chromium's client tag hash.
        let r = get(&mut db, user, "", &[(EXTENSIONS, "")]);
        let birthday = r.store_birthday.clone().unwrap();
        let ents = r.get_updates.as_ref().unwrap().entries.clone();
        assert_eq!(ents.len(), 1);
        let e = &ents[0];
        assert_eq!(e.client_tag_hash.as_deref(), Some(client_tag_hash(EXTENSIONS, dark).as_str()));
        let spec = e.specifics.clone().unwrap();
        assert!(spec.windows(dark.len()).any(|w| w == dark.as_bytes()));
        assert!(spec.windows(7).any(|w| w == b"4.9.133"));
        let tok = token(&r, EXTENSIONS);
        // Nothing new on the next poll.
        let r2 = get(&mut db, user, &birthday, &[(EXTENSIONS, &tok)]);
        assert!(r2.get_updates.unwrap().entries.is_empty());
        // The assignment goes: the server takes back what it added.
        crate::extstore::unassign(&db, dark, "everyone", "").unwrap();
        let r3 = get(&mut db, user, &birthday, &[(EXTENSIONS, &tok)]);
        let gone = r3.get_updates.unwrap().entries;
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].deleted, Some(true));
        // An extension the person synced themselves is never removed by the server.
        assert!(crate::extstore::synced_by_server(&db, user).unwrap().is_empty());
    }

    #[test]
    fn client_tag_hash_matches_chromium() {
        // components/sync/base/client_tag_hash_unittest.cc (PREFERENCES = field 37702).
        assert_eq!(client_tag_hash(37702, "tag1"), "iNFQtRFQb+IZcn1kKUJEZDDkLs4=");
        assert_eq!(client_tag_hash(37702, "tag2"), "XYxkF7bhS4eItStFgiOIAU23swI=");
    }

    #[test]
    fn users_do_not_see_each_other() {
        let (mut db, a) = db();
        let b = crate::store::add_user(&db, "guest", "another password", false).unwrap().id;
        let ba = get(&mut db, a, "", &[(45873, "")]).store_birthday.unwrap();
        let pw = SyncEntity { id_string: Some("c1".into()), version: Some(0), client_tag_hash: Some("h".into()), specifics: Some(default_specifics(45873)), ..Default::default() };
        ok(handle(&mut db, a, &commit_msg(&ba, vec![pw])).unwrap());
        assert!(get(&mut db, b, "", &[(45873, "")]).get_updates.unwrap().entries.is_empty());
        assert_eq!(get(&mut db, a, &ba, &[(45873, "")]).get_updates.unwrap().entries.len(), 1);
    }

    #[test]
    fn batches() {
        let (mut db, u) = db();
        let bday = get(&mut db, u, "", &[(45873, "")]).store_birthday.unwrap();
        let entries: Vec<SyncEntity> = (0..(MAX_BATCH + 20))
            .map(|i| SyncEntity { id_string: Some(format!("c{i}")), version: Some(0), client_tag_hash: Some(format!("h{i}")), specifics: Some(default_specifics(45873)), ..Default::default() })
            .collect();
        ok(handle(&mut db, u, &commit_msg(&bday, entries)).unwrap());
        let r = get(&mut db, u, &bday, &[(45873, "")]);
        let gu = r.get_updates.clone().unwrap();
        assert_eq!((gu.entries.len(), gu.changes_remaining), (MAX_BATCH, Some(20)));
        let r2 = get(&mut db, u, &bday, &[(45873, &token(&r, 45873))]);
        assert_eq!(r2.get_updates.unwrap().entries.len(), 20);
    }
}

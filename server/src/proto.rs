//! The parts of Chromium's sync protocol (components/sync/protocol/*.proto, BSD licence, The
//! Chromium Authors) that the server reads and writes, with the same field numbers.
//!
//! What a data type stores (EntitySpecifics) and an item's position (UniquePosition) are kept
//! as the raw bytes the browser sent: the server never needs to look inside them (apart from
//! which data type they are, and the Nigori passphrase type), so new data types in new
//! Chromium versions work without changing the server.

use prost::Message;

#[derive(Clone, PartialEq, Message)]
pub struct SyncEntity {
    #[prost(string, optional, tag = "1")]
    pub id_string: Option<String>,
    #[prost(string, optional, tag = "2")]
    pub parent_id_string: Option<String>,
    #[prost(int64, optional, tag = "4")]
    pub version: Option<i64>,
    #[prost(int64, optional, tag = "5")]
    pub mtime: Option<i64>,
    #[prost(int64, optional, tag = "6")]
    pub ctime: Option<i64>,
    #[prost(string, optional, tag = "7")]
    pub name: Option<String>,
    #[prost(string, optional, tag = "8")]
    pub non_unique_name: Option<String>,
    #[prost(string, optional, tag = "10")]
    pub server_defined_unique_tag: Option<String>,
    #[prost(bool, optional, tag = "18")]
    pub deleted: Option<bool>,
    #[prost(string, optional, tag = "19")]
    pub originator_cache_guid: Option<String>,
    #[prost(string, optional, tag = "20")]
    pub originator_client_item_id: Option<String>,
    /// EntitySpecifics, as sent.
    #[prost(bytes = "vec", optional, tag = "21")]
    pub specifics: Option<Vec<u8>>,
    #[prost(bool, optional, tag = "22")]
    pub folder: Option<bool>,
    #[prost(string, optional, tag = "23")]
    pub client_tag_hash: Option<String>,
    /// UniquePosition, as sent.
    #[prost(bytes = "vec", optional, tag = "25")]
    pub unique_position: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
pub struct DataTypeProgressMarker {
    #[prost(int32, optional, tag = "1")]
    pub data_type_id: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "2")]
    pub token: Option<Vec<u8>>,
    #[prost(message, optional, tag = "6")]
    pub gc_directive: Option<GarbageCollectionDirective>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GarbageCollectionDirective {
    #[prost(int64, optional, tag = "2")]
    pub version_watermark: Option<i64>,
    #[prost(bool, optional, tag = "6")]
    pub clear_metadata: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
pub struct CommitMessage {
    #[prost(message, repeated, tag = "1")]
    pub entries: Vec<SyncEntity>,
    #[prost(string, optional, tag = "2")]
    pub cache_guid: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetUpdatesMessage {
    #[prost(message, repeated, tag = "6")]
    pub from_progress_marker: Vec<DataTypeProgressMarker>,
    #[prost(bool, optional, tag = "8")]
    pub need_encryption_key: Option<bool>,
}

/// ClientToServerMessage.Contents
pub const COMMIT: i32 = 1;
pub const GET_UPDATES: i32 = 2;
pub const CLEAR_SERVER_DATA: i32 = 5;

#[derive(Clone, PartialEq, Message)]
pub struct ClientToServerMessage {
    #[prost(string, optional, tag = "1")]
    pub share: Option<String>,
    #[prost(int32, optional, tag = "2")]
    pub protocol_version: Option<i32>,
    #[prost(int32, optional, tag = "3")]
    pub message_contents: Option<i32>,
    #[prost(message, optional, tag = "4")]
    pub commit: Option<CommitMessage>,
    #[prost(message, optional, tag = "5")]
    pub get_updates: Option<GetUpdatesMessage>,
    #[prost(string, optional, tag = "7")]
    pub store_birthday: Option<String>,
}

/// CommitResponse.ResponseType
pub const RESP_SUCCESS: i32 = 1;
pub const RESP_CONFLICT: i32 = 2;
pub const RESP_INVALID_MESSAGE: i32 = 4;

/// A proto2 group (CommitResponse.EntryResponse = 1).
#[derive(Clone, PartialEq, Message)]
pub struct EntryResponse {
    #[prost(int32, optional, tag = "2")]
    pub response_type: Option<i32>,
    #[prost(string, optional, tag = "3")]
    pub id_string: Option<String>,
    #[prost(int64, optional, tag = "6")]
    pub version: Option<i64>,
    #[prost(string, optional, tag = "9")]
    pub error_message: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct CommitResponse {
    #[prost(group, repeated, tag = "1")]
    pub entryresponse: Vec<EntryResponse>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetUpdatesResponse {
    #[prost(message, repeated, tag = "1")]
    pub entries: Vec<SyncEntity>,
    #[prost(int64, optional, tag = "4")]
    pub changes_remaining: Option<i64>,
    #[prost(message, repeated, tag = "5")]
    pub new_progress_marker: Vec<DataTypeProgressMarker>,
    #[prost(bytes = "vec", repeated, tag = "6")]
    pub encryption_keys: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ClientCommand {
    #[prost(int32, optional, tag = "1")]
    pub set_sync_poll_interval: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ClearServerDataResponse {}

/// SyncEnums.ErrorType
pub const ERR_SUCCESS: i32 = 0;
pub const ERR_NOT_MY_BIRTHDAY: i32 = 2;

#[derive(Clone, PartialEq, Message)]
pub struct ClientToServerResponse {
    #[prost(message, optional, tag = "1")]
    pub commit: Option<CommitResponse>,
    #[prost(message, optional, tag = "2")]
    pub get_updates: Option<GetUpdatesResponse>,
    #[prost(int32, optional, tag = "4")]
    pub error_code: Option<i32>,
    #[prost(string, optional, tag = "5")]
    pub error_message: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub store_birthday: Option<String>,
    #[prost(message, optional, tag = "7")]
    pub client_command: Option<ClientCommand>,
    #[prost(message, optional, tag = "15")]
    pub clear_server_data: Option<ClearServerDataResponse>,
}

/// EntitySpecifics field numbers the server cares about.
pub const BOOKMARKS: i32 = 32904;
pub const NIGORI: i32 = 47745;
pub const EXTENSIONS: i32 = 48119;
/// EntitySpecifics.encrypted: present on encrypted data alongside the type's (empty) field.
const ENCRYPTED: u32 = 1;
/// NigoriSpecifics.passphrase_type, and its KEYSTORE_PASSPHRASE value.
const NIGORI_PASSPHRASE_TYPE: u32 = 30;
pub const KEYSTORE_PASSPHRASE: u64 = 2;

/// The top-level fields of a protobuf message: (field number, wire type, value bytes or varint).
fn fields(mut b: &[u8]) -> Vec<(u32, u8, Vec<u8>, u64)> {
    let mut out = vec![];
    while !b.is_empty() {
        let Ok(key) = prost::encoding::decode_varint(&mut b) else { break };
        let (field, wire) = ((key >> 3) as u32, (key & 7) as u8);
        match wire {
            0 => {
                let Ok(v) = prost::encoding::decode_varint(&mut b) else { break };
                out.push((field, wire, vec![], v));
            }
            1 => {
                let Some(v) = b.get(..8) else { break };
                out.push((field, wire, v.to_vec(), 0));
                b = b.get(8..).unwrap_or_default();
            }
            2 => {
                let Ok(len) = prost::encoding::decode_varint(&mut b) else { break };
                let Some(v) = usize::try_from(len).ok().and_then(|l| b.get(..l)) else { break };
                out.push((field, wire, v.to_vec(), 0));
                b = b.get(v.len()..).unwrap_or_default();
            }
            5 => {
                let Some(v) = b.get(..4) else { break };
                out.push((field, wire, v.to_vec(), 0));
                b = b.get(4..).unwrap_or_default();
            }
            _ => break,
        }
    }
    out
}

/// Which data type an EntitySpecifics is (its field number), if any.
pub fn data_type_of(specifics: &[u8]) -> Option<i32> {
    fields(specifics).into_iter().map(|f| f.0).find(|f| *f != ENCRYPTED && *f > 1).and_then(|f| i32::try_from(f).ok())
}

/// An EntitySpecifics with just the type's empty field (Chromium's AddDefaultFieldValue).
pub fn default_specifics(data_type: i32) -> Vec<u8> {
    let mut out = vec![];
    prost::encoding::encode_key(data_type as u32, prost::encoding::WireType::LengthDelimited, &mut out);
    prost::encoding::encode_varint(0, &mut out);
    out
}

/// The Nigori node's passphrase type (1, implicit, when not set).
pub fn nigori_passphrase_type(specifics: &[u8]) -> u64 {
    let nigori = fields(specifics).into_iter().find(|f| f.0 == NIGORI as u32).map(|f| f.2).unwrap_or_default();
    fields(&nigori).into_iter().find(|f| f.0 == NIGORI_PASSPHRASE_TYPE && f.1 == 0).map(|f| f.3).unwrap_or(1)
}

/// EntitySpecifics holding an ExtensionSpecifics (components/sync/protocol/extension_specifics.proto:
/// id 1, version 2, update_url 3, enabled 4, incognito_enabled 5).
pub fn extension_specifics(id: &str, version: &str, update_url: &str) -> Vec<u8> {
    use prost::encoding::{bool as pbool, encode_key, encode_varint, string, WireType};
    let mut inner = vec![];
    string::encode(1, &id.to_string(), &mut inner);
    string::encode(2, &version.to_string(), &mut inner);
    string::encode(3, &update_url.to_string(), &mut inner);
    pbool::encode(4, &true, &mut inner);
    pbool::encode(5, &false, &mut inner);
    let mut out = vec![];
    encode_key(EXTENSIONS as u32, WireType::LengthDelimited, &mut out);
    encode_varint(inner.len() as u64, &mut out);
    out.extend_from_slice(&inner);
    out
}

/// Chromium's ClientTagHash::FromUnhashed: base64(SHA-1(empty EntitySpecifics of the type + tag)).
pub fn client_tag_hash(data_type: i32, tag: &str) -> String {
    use base64::Engine;
    use sha1::Digest;
    let mut input = default_specifics(data_type);
    input.extend_from_slice(tag.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(sha1::Sha1::digest(&input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specifics_types() {
        assert_eq!(data_type_of(&default_specifics(BOOKMARKS)), Some(BOOKMARKS));
        // Encrypted: the encrypted field (1) and the type's empty field.
        let mut enc = vec![0x0a, 0x02, 0x08, 0x01];
        enc.extend(default_specifics(45873));
        assert_eq!(data_type_of(&enc), Some(45873));
        assert_eq!(data_type_of(&[]), None);
        assert_eq!(data_type_of(&[0xff, 0xff]), None, "junk is no type");
        // nigori { passphrase_type: 2 }
        let mut inner = vec![];
        prost::encoding::encode_key(30, prost::encoding::WireType::Varint, &mut inner);
        prost::encoding::encode_varint(2, &mut inner);
        let mut n2 = vec![];
        prost::encoding::encode_key(NIGORI as u32, prost::encoding::WireType::LengthDelimited, &mut n2);
        prost::encoding::encode_varint(inner.len() as u64, &mut n2);
        n2.extend(inner);
        assert_eq!(nigori_passphrase_type(&n2), KEYSTORE_PASSPHRASE);
        assert_eq!(nigori_passphrase_type(&default_specifics(NIGORI)), 1);
    }

    #[test]
    fn entry_response_is_a_group() {
        let r = CommitResponse { entryresponse: vec![EntryResponse { response_type: Some(1), id_string: Some("x".into()), version: Some(3), error_message: None }] };
        let b = r.encode_to_vec();
        // Field 1, wire type 3 (start group) = 0x0b.
        assert_eq!(b.first(), Some(&0x0b));
        assert_eq!(CommitResponse::decode(b.as_slice()).ok(), Some(r));
    }
}


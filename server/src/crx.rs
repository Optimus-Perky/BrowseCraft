//! CRX3 packages (what the Chrome Web Store serves): split the signed header from the zip,
//! check every signature in it, check the extension ID is the one its developer key gives,
//! and read the manifest (name, version, permissions) out of the zip.
//!
//! Layout: "Cr24", u32 version 3, u32 header length, a CrxFileHeader protobuf, then the zip.
//! Each proof signs  "CRX3 SignedData\0" + u32 len(signed_header_data) + signed_header_data + zip.
//! The ID is the first 16 bytes of SHA-256(developer public key), written with the letters a-p.

use std::io::{Cursor, Read};

use prost::Message;
use ring::signature;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq, Message)]
struct AsymmetricKeyProof {
    #[prost(bytes = "vec", optional, tag = "1")]
    public_key: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "2")]
    signature: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
struct CrxFileHeader {
    #[prost(message, repeated, tag = "2")]
    sha256_with_rsa: Vec<AsymmetricKeyProof>,
    #[prost(message, repeated, tag = "3")]
    sha256_with_ecdsa: Vec<AsymmetricKeyProof>,
    #[prost(bytes = "vec", optional, tag = "10000")]
    signed_header_data: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
struct SignedData {
    #[prost(bytes = "vec", optional, tag = "1")]
    crx_id: Option<Vec<u8>>,
}

/// Google's Chrome Web Store publisher key: every package the store serves carries a proof made
/// with it, besides the developer's own. (SHA-256 of the SubjectPublicKeyInfo, from Chromium's
/// components/crx_file/crx_verifier.cc kPublisherKeyHash.)
const PUBLISHER_KEY_HASH: [u8; 32] = [
    0x61, 0xf7, 0xf2, 0xa6, 0xbf, 0xcf, 0x74, 0xcd, 0x0b, 0xc1, 0xfe, 0x24, 0x97, 0xcc, 0x9b, 0x04, 0x25, 0x4c, 0x65,
    0x8f, 0x79, 0xf2, 0x14, 0x53, 0x92, 0x86, 0x7e, 0xa8, 0x36, 0x63, 0x67, 0xcf,
];

#[derive(Debug, Clone)]
pub struct Crx {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// "permissions", "optional_permissions" and host permissions, as the manifest lists them.
    pub permissions: Vec<String>,
    pub manifest: Value,
    /// Signed by Google's store key as well as the developer's.
    pub from_webstore: bool,
    pub sha256: String,
    pub size: usize,
}

fn u32_le(b: &[u8], at: usize) -> Result<u32, String> {
    let s: [u8; 4] = b.get(at..at + 4).and_then(|s| s.try_into().ok()).ok_or("truncated CRX")?;
    Ok(u32::from_le_bytes(s))
}

/// The extension ID a public key gives: SHA-256, first 16 bytes, each nibble as a letter a-p.
pub fn id_for_key(public_key: &[u8]) -> String {
    let digest = Sha256::digest(public_key);
    id_from_bytes(&digest[..16])
}

fn id_from_bytes(bytes: &[u8]) -> String {
    bytes.iter().flat_map(|b| [b >> 4, b & 0x0f]).map(|n| char::from(b'a' + n)).collect()
}

pub fn is_extension_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|c| (b'a'..=b'p').contains(&c))
}

/// One DER element: (tag, content, rest).
fn der(b: &[u8]) -> Result<(u8, &[u8], &[u8]), String> {
    let tag = *b.first().ok_or("short DER")?;
    let first = *b.get(1).ok_or("short DER")?;
    let (len, start) = if first < 0x80 {
        (first as usize, 2)
    } else {
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 {
            return Err("bad DER length".into());
        }
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | *b.get(2 + i).ok_or("short DER")? as usize;
        }
        (len, 2 + n)
    };
    let content = b.get(start..start + len).ok_or("short DER")?;
    Ok((tag, content, &b[start + len..]))
}

/// The key bits inside a SubjectPublicKeyInfo: PKCS#1 RSAPublicKey for RSA, the point for EC.
fn spki_key_bits(spki: &[u8]) -> Result<&[u8], String> {
    let (tag, seq, _) = der(spki)?;
    if tag != 0x30 {
        return Err("public key is not a DER sequence".into());
    }
    let (_, _algorithm, rest) = der(seq)?;
    let (tag, bits, _) = der(rest)?;
    if tag != 0x03 || bits.first() != Some(&0) {
        return Err("public key has no bit string".into());
    }
    Ok(&bits[1..])
}

pub fn parse(data: &[u8]) -> Result<Crx, String> {
    if data.get(..4) != Some(b"Cr24") {
        return Err("not a CRX file".into());
    }
    if u32_le(data, 4)? != 3 {
        return Err("only CRX3 packages are supported".into());
    }
    let header_len = u32_le(data, 8)? as usize;
    let header_bytes = data.get(12..12 + header_len).ok_or("truncated CRX header")?;
    let archive = &data[12 + header_len..];
    let header = CrxFileHeader::decode(header_bytes).map_err(|e| format!("bad CRX header: {e}"))?;
    let signed = header.signed_header_data.clone().ok_or("CRX header is not signed")?;
    let crx_id = SignedData::decode(signed.as_slice()).map_err(|e| format!("bad signed data: {e}"))?.crx_id.ok_or("no ID in the CRX")?;
    let id = id_from_bytes(&crx_id);

    let mut message = Vec::with_capacity(16 + 4 + signed.len() + archive.len());
    message.extend_from_slice(b"CRX3 SignedData\x00");
    message.extend_from_slice(&(signed.len() as u32).to_le_bytes());
    message.extend_from_slice(&signed);
    message.extend_from_slice(archive);

    let mut developer_key_ok = false;
    let mut from_webstore = false;
    let proofs = header.sha256_with_rsa.iter().map(|p| (p, true)).chain(header.sha256_with_ecdsa.iter().map(|p| (p, false)));
    let mut count = 0;
    for (proof, rsa) in proofs {
        count += 1;
        let key = proof.public_key.as_deref().ok_or("a proof has no key")?;
        let sig = proof.signature.as_deref().ok_or("a proof has no signature")?;
        let bits = spki_key_bits(key)?;
        // Old extensions (Grammarly, 2026) still sign with 1024-bit RSA developer keys, which Chromium accepts.
        let alg: &dyn signature::VerificationAlgorithm =
            if rsa { &signature::RSA_PKCS1_1024_8192_SHA256_FOR_LEGACY_USE_ONLY } else { &signature::ECDSA_P256_SHA256_ASN1 };
        signature::UnparsedPublicKey::new(alg, bits)
            .verify(&message, sig)
            .map_err(|_| format!("a signature in {id} does not verify: the file is damaged or altered"))?;
        if id_for_key(key) == id {
            developer_key_ok = true;
        }
        if Sha256::digest(key).as_slice() == PUBLISHER_KEY_HASH {
            from_webstore = true;
        }
    }
    if count == 0 {
        return Err("the CRX has no signatures".into());
    }
    if !developer_key_ok {
        return Err(format!("no signature in the CRX comes from the key that makes its ID {id}"));
    }

    let manifest = read_manifest(archive)?;
    let version = manifest.get("version").and_then(Value::as_str).ok_or("the manifest has no version")?.to_string();
    let name = localised(archive, &manifest, "name");
    let description = localised(archive, &manifest, "description");
    let mut permissions = Vec::new();
    for key in ["permissions", "optional_permissions", "host_permissions", "optional_host_permissions"] {
        if let Some(list) = manifest.get(key).and_then(Value::as_array) {
            permissions.extend(list.iter().filter_map(Value::as_str).map(str::to_string));
        }
    }
    Ok(Crx {
        id,
        name,
        version,
        description,
        permissions,
        manifest,
        from_webstore,
        sha256: hex(&Sha256::digest(data)),
        size: data.len(),
    })
}

/// The zip inside a CRX3 (after the signed header).
pub fn archive(data: &[u8]) -> Option<&[u8]> {
    if data.get(..4) != Some(b"Cr24") {
        return None;
    }
    let len = u32::from_le_bytes(data.get(8..12)?.try_into().ok()?) as usize;
    data.get(12 + len..)
}

/// The largest icon the manifest names, as (bytes, content type).
pub fn icon(data: &[u8], manifest: &Value) -> Option<(Vec<u8>, &'static str)> {
    let zip = archive(data)?;
    let icons = manifest.get("icons")?.as_object()?;
    let (_, path) = icons.iter().filter_map(|(k, v)| Some((k.parse::<u32>().ok()?, v.as_str()?))).max_by_key(|(k, _)| *k)?;
    let bytes = zip_file(zip, path.trim_start_matches('/'))?;
    let kind = if path.ends_with(".svg") { "image/svg+xml" } else if path.ends_with(".jpg") || path.ends_with(".jpeg") { "image/jpeg" } else { "image/png" };
    Some((bytes, kind))
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn zip_file(archive: &[u8], name: &str) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).ok()?;
    let mut f = zip.by_name(name).ok()?;
    let mut out = Vec::new();
    f.read_to_end(&mut out).ok()?;
    Some(out)
}

fn read_manifest(archive: &[u8]) -> Result<Value, String> {
    let raw = zip_file(archive, "manifest.json").ok_or("the CRX has no manifest.json")?;
    let text = String::from_utf8_lossy(&raw);
    // Some manifests start with a byte-order mark.
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("manifest.json does not parse: {e}"))
}

/// A manifest string, with "__MSG_x__" looked up in the default locale's messages.json.
fn localised(archive: &[u8], manifest: &Value, key: &str) -> String {
    let raw = manifest.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let Some(msg) = raw.strip_prefix("__MSG_").and_then(|s| s.strip_suffix("__")) else {
        return raw;
    };
    let locale = manifest.get("default_locale").and_then(Value::as_str).unwrap_or("en");
    let messages = zip_file(archive, &format!("_locales/{locale}/messages.json"))
        .and_then(|b| serde_json::from_str::<Value>(String::from_utf8_lossy(&b).trim_start_matches('\u{feff}')).ok());
    messages
        .and_then(|m| {
            m.as_object()?.iter().find(|(k, _)| k.eq_ignore_ascii_case(msg)).and_then(|(_, v)| v.get("message")?.as_str().map(str::to_string))
        })
        .unwrap_or(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_use_the_letters_a_to_p() {
        assert_eq!(id_from_bytes(&[0x00, 0x1f, 0xff]), "aabppp");
        assert!(is_extension_id("ddkjiahejlhfcafbddmgiahcphecmpfh"));
        assert!(!is_extension_id("ddkjiahejlhfcafbddmgiahcphecmpfz"));
    }

    #[test]
    fn rejects_what_is_not_a_crx3() {
        assert!(parse(b"PK\x03\x04").is_err());
        assert!(parse(b"Cr24\x02\x00\x00\x00").is_err());
    }

    /// Real packages from the Chrome Web Store, when BROWSECRAFT_TEST_CRX_DIR points at a folder of them.
    #[test]
    fn real_webstore_packages() -> Result<(), String> {
        let Ok(dir) = std::env::var("BROWSECRAFT_TEST_CRX_DIR") else { return Ok(()) };
        let mut seen = 0;
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("crx") {
                continue;
            }
            let data = std::fs::read(&path).map_err(|e| e.to_string())?;
            let crx = parse(&data)?;
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            assert_eq!(crx.id, stem, "ID from the signature matches the file name");
            assert!(crx.from_webstore, "{stem} carries Google's store signature");
            eprintln!("{} {} {} ({} permissions)", crx.id, crx.name, crx.version, crx.permissions.len());
            // One flipped byte in the zip must break the signatures.
            let mut bad = data.clone();
            let last = bad.len() - 10;
            bad[last] ^= 0x01;
            assert!(parse(&bad).is_err(), "an altered package is refused");
            seen += 1;
        }
        assert!(seen > 0, "no .crx files in {dir}");
        Ok(())
    }
}

//! Datagram framing and authentication (docs/surface-protocol.md, section 2).
//!
//! Wire layout: `MTX1 <mac> <json>` where `<mac>` is the first 16 bytes of
//! HMAC-SHA256(key, json bytes) as 32 lower-case hex characters. The MAC covers the JSON bytes
//! exactly as sent, so no canonicalisation is needed on either side.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

pub const MAGIC: &str = "MTX1";
pub const KEY_LEN: usize = 32;
/// Largest datagram the plugin accepts from a service.
pub const MAX_TO_PLUGIN: usize = 512;
/// Largest datagram a service accepts from the plugin (the welcome carries key lists).
pub const MAX_FROM_PLUGIN: usize = 1024;

#[derive(Clone)]
pub struct Key(pub [u8; KEY_LEN]);

impl Key {
    pub fn from_hex(s: &str) -> Result<Key, String> {
        let bytes = hex::decode(s.trim()).map_err(|e| format!("key is not hex: {e}"))?;
        if bytes.len() != KEY_LEN {
            return Err(format!("key must be {KEY_LEN} bytes ({} hex characters), got {}", KEY_LEN * 2, bytes.len()));
        }
        let mut k = [0u8; KEY_LEN];
        k.copy_from_slice(&bytes);
        Ok(Key(k))
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Key(<redacted>)")
    }
}

pub fn mac_hex(key: &Key, text: &[u8]) -> String {
    let mut m = Hmac::<Sha256>::new_from_slice(&key.0).expect("HMAC accepts any key length");
    m.update(text);
    let out = m.finalize().into_bytes();
    hex::encode(&out[..16])
}

/// `MTX1 <mac> <json>` for the given JSON text.
pub fn frame(key: &Key, json: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(json.len() + 40);
    out.extend_from_slice(MAGIC.as_bytes());
    out.push(b' ');
    out.extend_from_slice(mac_hex(key, json.as_bytes()).as_bytes());
    out.push(b' ');
    out.extend_from_slice(json.as_bytes());
    out
}

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    Oversized,
    BadMagic,
    BadFrame,
    BadMac,
}

/// Verifies the MAC and returns the JSON text. Nothing past the MAC is looked at before it verifies.
pub fn unframe<'a>(key: &Key, data: &'a [u8], max_len: usize) -> Result<&'a str, FrameError> {
    if data.len() > max_len {
        return Err(FrameError::Oversized);
    }
    if data.len() < 5 + 32 + 1 || &data[..5] != b"MTX1 " {
        return Err(FrameError::BadMagic);
    }
    let given = &data[5..37];
    if data[37] != b' ' || !given.iter().all(|b| b.is_ascii_hexdigit()) {
        return Err(FrameError::BadFrame);
    }
    let text = &data[38..];
    let expected = mac_hex(key, text);
    // Constant-time comparison of the two hex strings.
    let given_lower: Vec<u8> = given.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut diff = 0u8;
    for (a, b) in given_lower.iter().zip(expected.as_bytes()) {
        diff |= a ^ b;
    }
    if diff != 0 {
        return Err(FrameError::BadMac);
    }
    std::str::from_utf8(text).map_err(|_| FrameError::BadFrame)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> Key {
        Key::from_hex(&"0123456789abcdef".repeat(4)).unwrap()
    }

    #[test]
    fn hmac_matches_rfc4231_case_2_through_the_same_primitive() {
        // The plugin's pure-Lua HMAC is checked against the same vector, so both sides agree.
        let mut m = Hmac::<Sha256>::new_from_slice(b"Jefe").unwrap();
        m.update(b"what do ya want for nothing?");
        assert_eq!(hex::encode(m.finalize().into_bytes()), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    }

    #[test]
    fn frame_roundtrip() {
        let f = frame(&key(), r#"{"t":"hb"}"#);
        assert!(f.starts_with(b"MTX1 "));
        assert_eq!(unframe(&key(), &f, MAX_TO_PLUGIN).unwrap(), r#"{"t":"hb"}"#);
    }

    #[test]
    fn tampered_frames_are_refused_before_parsing() {
        let mut f = frame(&key(), r#"{"t":"hb"}"#);
        let n = f.len() - 1;
        f[n] ^= 1;
        assert_eq!(unframe(&key(), &f, MAX_TO_PLUGIN), Err(FrameError::BadMac));
        assert_eq!(unframe(&key(), b"OSC1 nope", MAX_TO_PLUGIN), Err(FrameError::BadMagic));
        assert_eq!(unframe(&key(), &vec![b'x'; 2000], MAX_TO_PLUGIN), Err(FrameError::Oversized));
        let other = Key::from_hex(&"ff".repeat(32)).unwrap();
        assert_eq!(unframe(&other, &frame(&key(), "{}"), MAX_TO_PLUGIN), Err(FrameError::BadMac));
    }

    #[test]
    fn key_validation() {
        assert!(Key::from_hex("abcd").is_err());
        assert!(Key::from_hex("zz").is_err());
        assert!(Key::from_hex(&"00".repeat(32)).is_ok());
    }
}

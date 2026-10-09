//! Datagram framing and authentication (docs/surface-protocol.md, section 2).
//!
//! Wire layout: `MTX1 <mac> <json>` where `<mac>` is SipHash-2-4 of the JSON bytes under the
//! first 16 bytes of the pairing key, as 16 lower-case hex characters. The MAC covers the JSON
//! bytes exactly as sent, so no canonicalisation is needed on either side.
//!
//! SipHash rather than HMAC-SHA256: the plugin computes the MAC in pure Lua inside onPC, where
//! HMAC-SHA256 was measured at 20 ms per packet (2026-10-09) and SipHash at a fraction of a
//! millisecond. SipHash-2-4 is a keyed PRF with a 128-bit key and a 64-bit tag; online forgery
//! attempts are bounded by the plugin's per-address throttle.

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

    fn sip_keys(&self) -> (u64, u64) {
        (u64::from_le_bytes(self.0[0..8].try_into().unwrap()), u64::from_le_bytes(self.0[8..16].try_into().unwrap()))
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Key(<redacted>)")
    }
}

/// SipHash-2-4 (Aumasson, Bernstein), 64-bit output.
pub fn siphash24(k0: u64, k1: u64, msg: &[u8]) -> u64 {
    let mut v0 = k0 ^ 0x736f6d6570736575;
    let mut v1 = k1 ^ 0x646f72616e646f6d;
    let mut v2 = k0 ^ 0x6c7967656e657261;
    let mut v3 = k1 ^ 0x7465646279746573;
    macro_rules! round {
        () => {
            v0 = v0.wrapping_add(v1);
            v1 = v1.rotate_left(13);
            v1 ^= v0;
            v0 = v0.rotate_left(32);
            v2 = v2.wrapping_add(v3);
            v3 = v3.rotate_left(16);
            v3 ^= v2;
            v0 = v0.wrapping_add(v3);
            v3 = v3.rotate_left(21);
            v3 ^= v0;
            v2 = v2.wrapping_add(v1);
            v1 = v1.rotate_left(17);
            v1 ^= v2;
            v2 = v2.rotate_left(32);
        };
    }
    let mut chunks = msg.chunks_exact(8);
    for c in &mut chunks {
        let m = u64::from_le_bytes(c.try_into().unwrap());
        v3 ^= m;
        round!();
        round!();
        v0 ^= m;
    }
    let mut last = (msg.len() as u64 & 0xff) << 56;
    for (i, b) in chunks.remainder().iter().enumerate() {
        last |= (*b as u64) << (8 * i);
    }
    v3 ^= last;
    round!();
    round!();
    v0 ^= last;
    v2 ^= 0xff;
    round!();
    round!();
    round!();
    round!();
    v0 ^ v1 ^ v2 ^ v3
}

pub fn mac_hex(key: &Key, text: &[u8]) -> String {
    let (k0, k1) = key.sip_keys();
    format!("{:016x}", siphash24(k0, k1, text))
}

/// `MTX1 <mac> <json>` for the given JSON text.
pub fn frame(key: &Key, json: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(json.len() + 24);
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

const MAC_HEX_LEN: usize = 16;

/// Verifies the MAC and returns the JSON text. Nothing past the MAC is looked at before it verifies.
pub fn unframe<'a>(key: &Key, data: &'a [u8], max_len: usize) -> Result<&'a str, FrameError> {
    if data.len() > max_len {
        return Err(FrameError::Oversized);
    }
    if data.len() < 5 + MAC_HEX_LEN + 1 || &data[..5] != b"MTX1 " {
        return Err(FrameError::BadMagic);
    }
    let given = &data[5..5 + MAC_HEX_LEN];
    if data[5 + MAC_HEX_LEN] != b' ' || !given.iter().all(|b| b.is_ascii_hexdigit()) {
        return Err(FrameError::BadFrame);
    }
    let text = &data[6 + MAC_HEX_LEN..];
    let expected = mac_hex(key, text);
    // Constant-time comparison of the two hex strings.
    let mut diff = 0u8;
    for (a, b) in given.iter().map(|b| b.to_ascii_lowercase()).zip(expected.as_bytes()) {
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
    fn siphash24_reference_vector() {
        // SipHash paper, Appendix A: k = 00..0f, m = 00..0e -> a129ca6149be45e5. The plugin's
        // pure-Lua implementation is checked against the same vector.
        let k: Vec<u8> = (0u8..16).collect();
        let m: Vec<u8> = (0u8..15).collect();
        let k0 = u64::from_le_bytes(k[0..8].try_into().unwrap());
        let k1 = u64::from_le_bytes(k[8..16].try_into().unwrap());
        assert_eq!(format!("{:016x}", siphash24(k0, k1, &m)), "a129ca6149be45e5");
        assert_eq!(format!("{:016x}", siphash24(k0, k1, &[])), "726fdb47dd0e0e31");
        assert_eq!(format!("{:016x}", siphash24(k0, k1, &(0u8..8).collect::<Vec<u8>>())), "93f5f5799a932462");
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

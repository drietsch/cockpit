//! Content hashing and stable id derivation (BLAKE3 everywhere, domain separated).

/// A 32-byte content hash.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentHash(pub [u8; 32]);

impl ContentHash {
    pub fn of(bytes: &[u8]) -> Self {
        ContentHash(*blake3::hash(bytes).as_bytes())
    }
    pub fn hex(&self) -> String {
        hex(&self.0)
    }
    pub fn short(&self) -> String {
        hex(&self.0[..8])
    }
    pub fn from_hex(s: &str) -> Option<Self> {
        if s.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hi = (chunk[0] as char).to_digit(16)?;
            let lo = (chunk[1] as char).to_digit(16)?;
            out[i] = (hi * 16 + lo) as u8;
        }
        Some(ContentHash(out))
    }
}

impl std::fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ContentHash({})", self.short())
    }
}

impl serde::Serialize for ContentHash {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> serde::Deserialize<'de> for ContentHash {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        ContentHash::from_hex(&s).ok_or_else(|| serde::de::Error::custom("invalid content hash"))
    }
}

pub fn hex(bytes: &[u8]) -> String {
    const T: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(T[(b >> 4) as usize] as char);
        s.push(T[(b & 15) as usize] as char);
    }
    s
}

/// First 16 hex characters of a domain-separated BLAKE3 over the given parts.
/// Parts are joined with NUL so that ("a","bc") and ("ab","c") differ.
pub fn short_id(domain: &str, parts: &[&str]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"cockpit:");
    h.update(domain.as_bytes());
    for p in parts {
        h.update(&[0]);
        h.update(p.as_bytes());
    }
    hex(&h.finalize().as_bytes()[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_id_is_stable_and_domain_separated() {
        assert_eq!(short_id("test", &["a", "b"]), short_id("test", &["a", "b"]));
        assert_ne!(short_id("test", &["a", "b"]), short_id("run", &["a", "b"]));
        assert_ne!(short_id("test", &["a", "bc"]), short_id("test", &["ab", "c"]));
        assert_eq!(short_id("x", &[]).len(), 16);
    }

    #[test]
    fn content_hash_round_trips_hex() {
        let h = ContentHash::of(b"hello");
        assert_eq!(ContentHash::from_hex(&h.hex()), Some(h));
    }
}

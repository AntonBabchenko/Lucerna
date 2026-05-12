//! Minecraft offline UUID derivation.
//!
//! This is `UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(UTF-8))`
//! in Java — MD5 of the input string, then force version-3 and
//! RFC-4122 variant bits. It is **not** standard UUIDv3 (which would
//! include a namespace in the hash). The vanilla launcher, MultiMC,
//! Prism, and HMCL all use this exact algorithm; offline servers
//! identify players by the resulting UUID.

use md5::{Digest, Md5};
use uuid::Uuid;

pub fn derive_offline_uuid(name: &str) -> Uuid {
    let input = format!("OfflinePlayer:{name}");
    let digest = Md5::digest(input.as_bytes());

    let mut bytes: [u8; 16] = digest.into();
    // Force version 3
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    // Force RFC 4122 variant
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vector test: the offline-derived UUID for the literal name "Notch".
    ///
    /// This is **not** Notch's real Mojang account UUID
    /// (`069a79f4-44e9-4726-a5be-fca90e38aaf5`) — that one is a v4 random
    /// UUID minted when Mojang created his account. Offline derivation
    /// gives a different value because it's a deterministic hash of
    /// `"OfflinePlayer:Notch"`.
    ///
    /// Reproduce in Java:
    ///   `UUID.nameUUIDFromBytes("OfflinePlayer:Notch".getBytes(UTF_8))`
    /// → `b50ad385-829d-3141-a216-7e7d7539ba7f`.
    /// HMCL, MultiMC, Prism all produce this same value.
    #[test]
    fn notch_offline_uuid_matches_known_vector() {
        let got = derive_offline_uuid("Notch");
        assert_eq!(got.to_string(), "b50ad385-829d-3141-a216-7e7d7539ba7f");
    }

    #[test]
    fn deterministic_same_input_same_output() {
        let a = derive_offline_uuid("Steve");
        let b = derive_offline_uuid("Steve");
        assert_eq!(a, b);
    }

    #[test]
    fn different_names_different_uuids() {
        let a = derive_offline_uuid("Alice");
        let b = derive_offline_uuid("Bob");
        assert_ne!(a, b);
    }

    #[test]
    fn empty_name_does_not_panic() {
        // Edge case — we don't validate at this layer; UI is expected
        // to refuse empty input. Just ensure no panic.
        let _ = derive_offline_uuid("");
    }

    #[test]
    fn version_bits_are_3() {
        let uuid = derive_offline_uuid("AnyName");
        let bytes = uuid.as_bytes();
        assert_eq!((bytes[6] & 0xf0) >> 4, 3);
    }

    #[test]
    fn variant_bits_are_rfc4122() {
        let uuid = derive_offline_uuid("AnyName");
        let bytes = uuid.as_bytes();
        assert_eq!((bytes[8] & 0xc0) >> 6, 0b10);
    }

    #[test]
    fn unicode_name_is_utf8_encoded() {
        let a = derive_offline_uuid("Игрок");
        let b = derive_offline_uuid("プレイヤー");
        assert_ne!(a, b);
    }
}

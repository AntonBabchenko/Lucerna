//! UUID v4 generation + validation for instance IDs.
//!
//! Same convention as `accounts::store::Account.id` — keeps the two
//! namespaces interoperable in maintenance scripts.

use uuid::Uuid;

/// Fresh UUID v4 as the canonical hyphenated string.
pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

/// True iff `s` is a parseable UUID v4 hyphenated string.
pub fn is_valid_uuid_v4(s: &str) -> bool {
    match Uuid::parse_str(s) {
        Ok(u) => u.get_version_num() == 4,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_id_is_36_chars_v4() {
        let id = new_id();
        assert_eq!(id.len(), 36);
        assert!(is_valid_uuid_v4(&id));
    }

    #[test]
    fn two_ids_differ() {
        assert_ne!(new_id(), new_id());
    }

    #[test]
    fn rejects_non_uuid() {
        assert!(!is_valid_uuid_v4(""));
        assert!(!is_valid_uuid_v4("not-a-uuid"));
        assert!(!is_valid_uuid_v4("3f4a"));
    }

    #[test]
    fn rejects_uuid_v1() {
        // A v1 UUID (timestamp-based). Hand-crafted to set version nibble = 1.
        let v1 = "550e8400-e29b-11d4-a716-446655440000";
        assert!(!is_valid_uuid_v4(v1));
    }
}

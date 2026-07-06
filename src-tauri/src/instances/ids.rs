//! UUID v4 generation for opaque internal tokens (e.g. the server-import
//! staging token).
//!
//! Instance and server *directory* ids are now human-readable slugs — see
//! [`crate::naming`]. This module remains for the few places that still need a
//! collision-free opaque token.

use uuid::Uuid;

/// Fresh UUID v4 as the canonical hyphenated string.
pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_id_is_36_char_hyphenated() {
        let id = new_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);
    }

    #[test]
    fn two_ids_differ() {
        assert_ne!(new_id(), new_id());
    }
}

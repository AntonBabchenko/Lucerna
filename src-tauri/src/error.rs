//! Top-level error enum. Every fallible function in the launcher returns
//! `Result<T>` (alias for `std::result::Result<T, Error>`).
//!
//! `Error` derives `Serialize` + `specta::Type` so each variant crosses
//! the IPC boundary with its context intact — the UI gets typed errors,
//! not strings.

use serde::Serialize;
use specta::Type;
use thiserror::Error as ThisError;

#[derive(Debug, ThisError, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Error {
    #[error("Network error fetching {url}: {details}")]
    Network { url: String, details: String },

    #[error("Hash mismatch for {path}: expected {expected}, got {got}")]
    HashMismatch {
        path: String,
        expected: String,
        got: String,
    },

    #[error("Java spawn failed: {details}")]
    JavaSpawn { details: String },

    #[error("Minecraft is already running")]
    AlreadyRunning,

    #[error("Account not set — enter your name first")]
    AccountNotSet,

    #[error("Version {id} not found in manifest")]
    UnknownVersion { id: String },

    #[error("{loader} does not support Minecraft {mc_version}")]
    LoaderUnavailable { loader: String, mc_version: String },

    #[error("Unsupported platform: {os}/{arch}")]
    UnsupportedPlatform { os: String, arch: String },

    #[error("IO error at {path}: {details}")]
    Io { path: String, details: String },

    #[error("Cannot delete the last instance — at least one must remain")]
    LastInstance,

    #[error("Active instance has no Minecraft version selected — pick one first")]
    NoVersionSelected,

    #[error("Instance {id} not found")]
    InstanceNotFound { id: String },
}

pub type Result<T> = std::result::Result<T, Error>;

// Convenience constructors for the most common conversions. Inline `?`
// at the call site is otherwise tedious because the variants want
// context strings.

impl Error {
    pub fn network(url: impl Into<String>, cause: impl std::fmt::Display) -> Self {
        Self::Network {
            url: url.into(),
            details: cause.to_string(),
        }
    }

    pub fn io(path: impl Into<String>, cause: impl std::fmt::Display) -> Self {
        Self::Io {
            path: path.into(),
            details: cause.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_constructor_includes_context() {
        let e = Error::network("https://example.com/x", "connection refused");
        let msg = format!("{e}");
        assert!(msg.contains("https://example.com/x"));
        assert!(msg.contains("connection refused"));
    }

    #[test]
    fn hash_mismatch_serializes_with_tag() {
        let e = Error::HashMismatch {
            path: "/tmp/x".into(),
            expected: "aaa".into(),
            got: "bbb".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        // tag: "kind" + snake_case rename → "hash_mismatch"
        assert!(json.contains(r#""kind":"hash_mismatch""#), "got: {json}");
        assert!(json.contains(r#""expected":"aaa""#));
    }

    #[test]
    fn loader_unavailable_serializes_with_tag() {
        let e = Error::LoaderUnavailable {
            loader: "fabric".into(),
            mc_version: "1.6.4".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains(r#""kind":"loader_unavailable""#), "got: {json}");
        assert!(json.contains(r#""loader":"fabric""#), "got: {json}");
        assert!(json.contains(r#""mc_version":"1.6.4""#), "got: {json}");
    }

    #[test]
    fn last_instance_serializes_with_tag() {
        let e = Error::LastInstance;
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains(r#""kind":"last_instance""#), "got: {json}");
    }

    #[test]
    fn no_version_selected_serializes_with_tag() {
        let e = Error::NoVersionSelected;
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains(r#""kind":"no_version_selected""#), "got: {json}");
    }

    #[test]
    fn instance_not_found_carries_id() {
        let e = Error::InstanceNotFound {
            id: "3f4a-bbbb".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains(r#""kind":"instance_not_found""#), "got: {json}");
        assert!(json.contains(r#""id":"3f4a-bbbb""#), "got: {json}");
    }
}

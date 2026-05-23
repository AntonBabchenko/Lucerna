//! Read/write `account.json` in the app data dir — schema v2 multi-account.
//!
//! Schema:
//! ```json
//! {
//!   "version": 2,
//!   "accounts": [{ "id", "name", "uuid", "expires_at" }, ...],
//!   "active_id": "..."
//! }
//! ```
//!
//! `expires_at` is `null` for offline accounts (never expire). Microsoft
//! accounts (deferred to a future slice, see git tag `v0.2.0-msauth-attempt`)
//! will set it to a unix timestamp; the launcher will refresh tokens when
//! `expires_at <= now + buffer`.
//!
//! v0.1.0 produced a different shape: `{ "name", "uuid" }` flat. The reader
//! recognises that and migrates it inline to v2 (one offline entry) — no
//! data loss for upgrades.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct Account {
    /// Local UUID v4 — disambiguates entries; key suffix for any per-account
    /// secret storage (currently unused; reserved for the deferred MS auth).
    pub id: String,
    /// Display name (MC username).
    pub name: String,
    /// MC UUID, canonical hyphenated form.
    pub uuid: String,
    /// Unix seconds. `None` for offline (never expires).
    pub expires_at: Option<f64>,
}

/// On-disk file format v2. The `version` discriminator makes future
/// migrations easy. Empty (no accounts, no active) is valid — first
/// boot for a new install.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountFile {
    pub version: u32,
    pub accounts: Vec<Account>,
    pub active_id: Option<String>,
}

impl Default for AccountFile {
    fn default() -> Self {
        Self {
            version: 2,
            accounts: Vec::new(),
            active_id: None,
        }
    }
}

/// v0.1.0 shape — only used to recognise the legacy file and convert it.
#[derive(Debug, Deserialize)]
struct LegacyV1 {
    name: String,
    uuid: String,
}

/// Read the account file. Three cases:
/// 1. Missing — return default empty v2.
/// 2. v2 (has `"version": 2`) — parse, return.
/// 3. v1 (no `version`, has `name` + `uuid`) — migrate in memory to v2,
///    persist back to disk, return.
pub fn read_account_file(file: &Path) -> Result<AccountFile> {
    let raw = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(AccountFile::default()),
        Err(e) => return Err(Error::io(file.display().to_string(), e)),
    };

    // Try v2 first.
    if let Ok(parsed) = serde_json::from_str::<AccountFile>(&raw) {
        if parsed.version == 2 {
            return Ok(parsed);
        }
    }

    // Try v1 (`{ name, uuid }`).
    if let Ok(v1) = serde_json::from_str::<LegacyV1>(&raw) {
        let migrated_id = format!("of-{}", uuid::Uuid::new_v4());
        let migrated = AccountFile {
            version: 2,
            accounts: vec![Account {
                id: migrated_id.clone(),
                name: v1.name,
                uuid: v1.uuid,
                expires_at: None,
            }],
            active_id: Some(migrated_id),
        };
        write_account_file(file, &migrated)?;
        return Ok(migrated);
    }

    Err(Error::io(
        file.display().to_string(),
        format!("parse: file is neither v2 nor v0.1.0 shape: {raw}"),
    ))
}

pub fn write_account_file(file: &Path, account_file: &AccountFile) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let json = serde_json::to_vec_pretty(account_file)
        .map_err(|e| Error::io(file.display().to_string(), format!("serialise: {e}")))?;
    std::fs::write(file, json).map_err(|e| Error::io(file.display().to_string(), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn account_serializes_with_id_uuid_name_and_null_expires() {
        let acc = Account {
            id: "of-xyz".into(),
            name: "Steve".into(),
            uuid: "b50ad385-829d-3141-a216-7e7d7539ba7f".into(),
            expires_at: None,
        };
        let json = serde_json::to_string(&acc).unwrap();
        assert!(json.contains(r#""id":"of-xyz""#), "got: {json}");
        assert!(json.contains(r#""name":"Steve""#));
        assert!(json.contains(r#""uuid":"b50ad385-829d-3141-a216-7e7d7539ba7f""#));
        assert!(json.contains(r#""expires_at":null"#));
    }

    #[test]
    fn read_v1_file_migrates_to_v2_in_memory_and_on_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("account.json");
        // v0.1.0 shape — flat { name, uuid }, no `version` key.
        std::fs::write(
            &path,
            r#"{"name":"OldUser","uuid":"b50ad385-829d-3141-a216-7e7d7539ba7f"}"#,
        )
        .unwrap();

        let file = read_account_file(&path).unwrap();
        assert_eq!(file.version, 2);
        assert_eq!(file.accounts.len(), 1);
        assert_eq!(file.accounts[0].name, "OldUser");
        assert_eq!(
            file.accounts[0].uuid,
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert!(file.accounts[0].id.starts_with("of-"));
        assert!(file.accounts[0].expires_at.is_none());
        assert_eq!(
            file.active_id.as_deref(),
            Some(file.accounts[0].id.as_str())
        );

        // The migrated form must have been persisted back to disk as v2.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(r#""version": 2"#), "got: {raw}");
        assert!(raw.contains(r#""accounts":"#));
        assert!(raw.contains(r#""active_id":"#));
    }

    #[test]
    fn read_v2_file_roundtrips_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("account.json");
        let original = AccountFile {
            version: 2,
            accounts: vec![
                Account {
                    id: "of-1".into(),
                    name: "Steve".into(),
                    uuid: "b50ad385-829d-3141-a216-7e7d7539ba7f".into(),
                    expires_at: None,
                },
                Account {
                    id: "of-2".into(),
                    name: "Alex".into(),
                    uuid: "ec561538-f3fd-461d-aff5-086b22154bce".into(),
                    expires_at: None,
                },
            ],
            active_id: Some("of-1".into()),
        };
        write_account_file(&path, &original).unwrap();
        let read_back = read_account_file(&path).unwrap();
        assert_eq!(read_back, original);
    }

    #[test]
    fn read_missing_file_returns_default_empty_v2() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        let file = read_account_file(&path).unwrap();
        assert_eq!(file.version, 2);
        assert!(file.accounts.is_empty());
        assert!(file.active_id.is_none());
    }

    #[test]
    fn malformed_json_reports_error_with_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, b"{not json").unwrap();
        let err = read_account_file(&path).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains(&path.display().to_string()), "got: {msg}");
        assert!(msg.contains("parse:"), "got: {msg}");
    }
}

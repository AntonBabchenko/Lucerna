//! Read/write `account.json` in the app data dir.
//!
//! The format is a JSON object with `{ "name": ..., "uuid": ... }`.
//! Offline accounts have nothing to protect — plain JSON is the
//! correct storage choice. Microsoft credentials will go to OS
//! keychain via the `keyring` crate in v0.2.0; that work does not
//! touch this file.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct Account {
    pub name: String,
    /// UUID as canonical hyphenated string ("xxxxxxxx-xxxx-...").
    pub uuid: String,
}

pub fn read_account(file: &Path) -> Result<Option<Account>> {
    let raw = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::io(file.display().to_string(), e)),
    };
    let account: Account = serde_json::from_slice(&raw)
        .map_err(|e| Error::io(file.display().to_string(), format!("parse: {e}")))?;
    Ok(Some(account))
}

pub fn write_account(file: &Path, account: &Account) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let json = serde_json::to_vec_pretty(account)
        .map_err(|e| Error::io(file.display().to_string(), format!("serialise: {e}")))?;
    std::fs::write(file, json).map_err(|e| Error::io(file.display().to_string(), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn read_returns_none_when_file_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("account.json");
        assert_eq!(read_account(&path).unwrap(), None);
    }

    #[test]
    fn roundtrip_preserves_account() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("account.json");
        let original = Account {
            name: "Steve".into(),
            uuid: "069a79f4-44e9-4726-a5be-fca90e38aaf5".into(),
        };
        write_account(&path, &original).unwrap();
        let read_back = read_account(&path).unwrap().unwrap();
        assert_eq!(read_back, original);
    }

    #[test]
    fn write_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a/b/c/account.json");
        let account = Account {
            name: "Alex".into(),
            uuid: "ec561538-f3fd-461d-aff5-086b22154bce".into(),
        };
        write_account(&nested, &account).unwrap();
        assert!(nested.exists());
    }

    #[test]
    fn malformed_json_reports_error_with_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, b"{not json").unwrap();
        let err = read_account(&path).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains(&path.display().to_string()), "got: {msg}");
        assert!(msg.contains("parse:"), "got: {msg}");
    }
}

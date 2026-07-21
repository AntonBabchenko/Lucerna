//! Read/write `account.json` in the app data dir — schema v3 multi-account.
//!
//! Schema:
//! ```json
//! {
//!   "version": 3,
//!   "accounts": [{ "id", "kind", "name", "uuid", "expires_at" }, ...],
//!   "active_id": "..."
//! }
//! ```
//!
//! `kind` is `"offline"` or `"microsoft"`. `expires_at` is `null` for offline
//! accounts (never expire). Microsoft accounts set it to a unix timestamp; the
//! launcher refreshes tokens when `expires_at <= now + buffer`.
//!
//! v0.2.0 produced v2 shape: `{ "version": 2, "accounts": [{ "id", "name",
//! "uuid", "expires_at" }], "active_id" }` — no `kind` field. Migrated to v3
//! by assigning `kind: Offline` to every entry.
//!
//! v0.1.0 produced a different shape: `{ "name", "uuid" }` flat. The reader
//! recognises that and migrates it inline to v3 (one offline entry) — no
//! data loss for upgrades.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::Path;

/// Discriminator for account type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    Offline,
    Microsoft,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct Account {
    /// Local UUID v4 — disambiguates entries; used as keychain key suffix
    /// for Microsoft accounts to namespace their refresh_token + mc_access_token.
    pub id: String,
    /// Discriminator. Offline accounts have nothing to protect; Microsoft
    /// accounts have secrets stored in OS keyring (see `accounts/keychain.rs`).
    pub kind: AccountKind,
    /// Display name (MC username for both kinds).
    pub name: String,
    /// MC UUID, canonical hyphenated form.
    pub uuid: String,
    /// Unix seconds. `None` for offline (never expires). Microsoft accounts
    /// set this to the MC access token expiry; the launcher refreshes when
    /// `expires_at <= now + 5 minutes`.
    pub expires_at: Option<f64>,
}

/// On-disk file format v3. The `version` discriminator makes future
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
            version: 3,
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

/// Read the account file. Four cases:
/// 1. Missing — return default empty v3.
/// 2. v3 (has `"version": 3`, has `kind` field) — parse, return.
/// 3. v2 (has `"version": 2`, no `kind` field) — migrate to v3 (kind: Offline),
///    persist back to disk, return.
/// 4. v1 (no `version`, has `name` + `uuid`) — migrate to v3,
///    persist back to disk, return.
///
/// A `"version": 3` file WITHOUT `kind` is corruption (strict parse error) —
/// the temporary rescue for the pre-merge dev-branch shape was retired once
/// every affected file had been rewritten to clean v3.
pub fn read_account_file(file: &Path) -> Result<AccountFile> {
    let raw = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(AccountFile::default()),
        Err(e) => return Err(Error::io(file.display().to_string(), e)),
    };

    // Try v3 first (current shape — has `kind` field).
    if let Ok(parsed) = serde_json::from_str::<AccountFile>(&raw) {
        if parsed.version == 3 {
            return Ok(parsed);
        }
    }

    // Try v2 shape (no `kind` field): true v2 files migrate to v3 with
    // `kind: Offline` on every entry (Microsoft accounts were never
    // expressible without kind). A kind-less `version == 3` file — once
    // rescued here for a pre-merge dev-branch artifact (PR #16 era) — is now
    // treated as corruption again: every machine that had one was rewritten
    // to clean v3 on first read long ago, and silently "rescuing" a future
    // buggy writer's output to Offline would mask real corruption.
    #[derive(Deserialize)]
    struct LegacyV2Account {
        id: String,
        name: String,
        uuid: String,
        expires_at: Option<f64>,
    }
    #[derive(Deserialize)]
    struct LegacyV2 {
        version: u32,
        accounts: Vec<LegacyV2Account>,
        active_id: Option<String>,
    }
    if let Ok(v2) = serde_json::from_str::<LegacyV2>(&raw) {
        if v2.version == 2 {
            let migrated = AccountFile {
                version: 3,
                accounts: v2
                    .accounts
                    .into_iter()
                    .map(|a| Account {
                        id: a.id,
                        kind: AccountKind::Offline,
                        name: a.name,
                        uuid: a.uuid,
                        expires_at: a.expires_at,
                    })
                    .collect(),
                active_id: v2.active_id,
            };
            write_account_file(file, &migrated)?;
            return Ok(migrated);
        }
    }

    // Try v1 (`{ name, uuid }`) — pre-existing path, emit v3 directly.
    if let Ok(v1) = serde_json::from_str::<LegacyV1>(&raw) {
        let migrated_id = format!("of-{}", uuid::Uuid::new_v4());
        let migrated = AccountFile {
            version: 3,
            accounts: vec![Account {
                id: migrated_id.clone(),
                kind: AccountKind::Offline,
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
        format!("parse: file is neither v3 nor v2 nor v0.1.0 shape: {raw}"),
    ))
}

/// Compute the upserted account file WITHOUT persisting it. If an entry with
/// this MC UUID already exists, update its `name` + `expires_at` (keeping the
/// existing `id` so the keyring entries stay associated). Otherwise append a
/// new `Microsoft` entry with a fresh `ms-<uuid_v4>` id and set it as active.
///
/// Returns the mutated `AccountFile` (caller persists it) plus the affected
/// `Account`. Splitting compute-from-persist lets the caller write the OS
/// keyring secrets FIRST, so a keyring failure never leaves a signed-in-looking
/// account.json entry with missing/stale tokens.
pub fn prepare_upsert_microsoft_account(
    file: &Path,
    mc_uuid: &str,
    name: &str,
    expires_at: Option<f64>,
) -> Result<(AccountFile, Account)> {
    let mut store = read_account_file(file)?;
    if let Some(existing) = store.accounts.iter_mut().find(|a| a.uuid == mc_uuid) {
        existing.name = name.to_string();
        existing.expires_at = expires_at;
        existing.kind = AccountKind::Microsoft;
        let updated = existing.clone();
        return Ok((store, updated));
    }
    let new_account = Account {
        id: format!("ms-{}", uuid::Uuid::new_v4()),
        kind: AccountKind::Microsoft,
        name: name.to_string(),
        uuid: mc_uuid.to_string(),
        expires_at,
    };
    store.accounts.push(new_account.clone());
    store.active_id = Some(new_account.id.clone());
    Ok((store, new_account))
}

/// Upsert a Microsoft account by MC UUID and persist. Thin wrapper over
/// `prepare_upsert_microsoft_account` for callers that don't need the
/// keyring-before-persist ordering.
pub fn upsert_microsoft_account(
    file: &Path,
    mc_uuid: &str,
    name: &str,
    expires_at: Option<f64>,
) -> Result<Account> {
    let (store, account) = prepare_upsert_microsoft_account(file, mc_uuid, name, expires_at)?;
    write_account_file(file, &store)?;
    Ok(account)
}

/// Compute an update of an EXISTING Microsoft account, located by its local
/// `id` (not by uuid), WITHOUT persisting. Used by the silent-refresh path:
/// unlike an upsert, a refresh must never resurrect an account the user has
/// since removed — if the id is gone, return an auth error so the caller can
/// treat the refresh as failed (and fall back to the stored token / prompt a
/// re-sign-in) rather than recreating a ghost entry.
pub fn prepare_refresh_microsoft_account_by_id(
    file: &Path,
    account_id: &str,
    mc_uuid: &str,
    name: &str,
    expires_at: Option<f64>,
) -> Result<(AccountFile, Account)> {
    let mut store = read_account_file(file)?;
    let Some(existing) = store.accounts.iter_mut().find(|a| a.id == account_id) else {
        return Err(Error::AuthFailed {
            stage: "refresh".into(),
            details: format!("account {account_id} no longer exists"),
        });
    };
    existing.name = name.to_string();
    existing.uuid = mc_uuid.to_string();
    existing.expires_at = expires_at;
    existing.kind = AccountKind::Microsoft;
    let updated = existing.clone();
    Ok((store, updated))
}

/// Atomic write: serialise to a sibling temp file, then rename over the
/// target. A crash or keyring failure mid-write can no longer leave a
/// truncated/half-written account.json on disk (matches the temp+rename
/// pattern in `servers::write_atomic`).
pub fn write_account_file(file: &Path, account_file: &AccountFile) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let json = serde_json::to_vec_pretty(account_file)
        .map_err(|e| Error::io(file.display().to_string(), format!("serialise: {e}")))?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    let renamed = std::fs::rename(&tmp, file).map_err(|e| Error::io(file.display().to_string(), e));
    if renamed.is_err() {
        // Don't leave a stale account.json.tmp behind on a failed rename.
        let _ = std::fs::remove_file(&tmp);
    }
    renamed
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn account_serializes_with_id_uuid_name_and_null_expires() {
        let acc = Account {
            id: "of-xyz".into(),
            kind: AccountKind::Offline,
            name: "Steve".into(),
            uuid: "b50ad385-829d-3141-a216-7e7d7539ba7f".into(),
            expires_at: None,
        };
        let json = serde_json::to_string(&acc).unwrap();
        assert!(json.contains(r#""id":"of-xyz""#), "got: {json}");
        assert!(json.contains(r#""kind":"offline""#), "got: {json}");
        assert!(json.contains(r#""name":"Steve""#));
        assert!(json.contains(r#""uuid":"b50ad385-829d-3141-a216-7e7d7539ba7f""#));
        assert!(json.contains(r#""expires_at":null"#));
    }

    #[test]
    fn read_v1_file_migrates_to_v3_in_memory_and_on_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("account.json");
        // v0.1.0 shape — flat { name, uuid }, no `version` key.
        std::fs::write(
            &path,
            r#"{"name":"OldUser","uuid":"b50ad385-829d-3141-a216-7e7d7539ba7f"}"#,
        )
        .unwrap();

        let file = read_account_file(&path).unwrap();
        assert_eq!(file.version, 3);
        assert_eq!(file.accounts.len(), 1);
        assert_eq!(file.accounts[0].kind, AccountKind::Offline);
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

        // The migrated form must have been persisted back to disk as v3.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(r#""version": 3"#), "got: {raw}");
        assert!(raw.contains(r#""accounts":"#));
        assert!(raw.contains(r#""active_id":"#));
    }

    #[test]
    fn read_v3_file_roundtrips_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("account.json");
        let original = AccountFile {
            version: 3,
            accounts: vec![
                Account {
                    id: "of-1".into(),
                    kind: AccountKind::Offline,
                    name: "Steve".into(),
                    uuid: "b50ad385-829d-3141-a216-7e7d7539ba7f".into(),
                    expires_at: None,
                },
                Account {
                    id: "of-2".into(),
                    kind: AccountKind::Offline,
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
    fn read_missing_file_returns_default_empty_v3() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        let file = read_account_file(&path).unwrap();
        assert_eq!(file.version, 3);
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

    #[test]
    fn read_v2_file_migrates_to_v3_setting_kind_offline() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("account.json");
        // v2 shape: { version: 2, accounts: [{ id, name, uuid, expires_at }], active_id }.
        // No `kind` field anywhere — the migration must default it to Offline.
        std::fs::write(
            &path,
            r#"{"version":2,"accounts":[{"id":"of-1","name":"Steve","uuid":"b50ad385-829d-3141-a216-7e7d7539ba7f","expires_at":null}],"active_id":"of-1"}"#,
        )
        .unwrap();

        let file = read_account_file(&path).unwrap();
        assert_eq!(file.version, 3);
        assert_eq!(file.accounts.len(), 1);
        assert_eq!(file.accounts[0].kind, AccountKind::Offline);
        assert_eq!(file.accounts[0].name, "Steve");
    }

    /// A kind-less `"version": 3` file is corruption and must be a strict
    /// parse error. (A temporary rescue once migrated this shape — written by
    /// a pre-merge dev branch, PR #16 era — but it was retired: every
    /// affected file was rewritten to clean v3 on first read, and silently
    /// coercing a future buggy writer's output to Offline would mask real
    /// corruption.)
    #[test]
    fn read_v3_file_without_kind_is_a_parse_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("account.json");
        std::fs::write(
            &path,
            r#"{"version":3,"accounts":[{"id":"of-dde68bac-ebde-4988-96ca-dc6f89d344bc","name":"Reiner","uuid":"1b8e1760-3294-301a-84dc-ac5d2e45a45d","expires_at":null}],"active_id":"of-dde68bac-ebde-4988-96ca-dc6f89d344bc"}"#,
        )
        .unwrap();

        let err = read_account_file(&path).unwrap_err();
        assert!(
            err.to_string().contains("parse"),
            "kind-less v3 must surface as a parse error, got: {err}"
        );
    }

    #[test]
    fn upsert_microsoft_account_creates_new_when_uuid_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("account.json");
        let initial = AccountFile::default();
        write_account_file(&path, &initial).unwrap();

        let added = upsert_microsoft_account(
            &path,
            "550e8400-e29b-41d4-a716-446655440000",
            "Notch",
            Some(1_700_000_000.0),
        )
        .unwrap();

        assert_eq!(added.kind, AccountKind::Microsoft);
        assert_eq!(added.name, "Notch");
        assert_eq!(added.uuid, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(added.expires_at, Some(1_700_000_000.0));
        assert!(added.id.starts_with("ms-"));

        let on_disk = read_account_file(&path).unwrap();
        assert_eq!(on_disk.accounts.len(), 1);
        assert_eq!(on_disk.active_id.as_deref(), Some(added.id.as_str()));
    }

    #[test]
    fn upsert_microsoft_account_updates_existing_by_uuid() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("account.json");
        let existing = Account {
            id: "ms-original".into(),
            kind: AccountKind::Microsoft,
            name: "OldName".into(),
            uuid: "550e8400-e29b-41d4-a716-446655440000".into(),
            expires_at: Some(100.0),
        };
        let initial = AccountFile {
            version: 3,
            accounts: vec![existing.clone()],
            active_id: Some("ms-original".into()),
        };
        write_account_file(&path, &initial).unwrap();

        let updated = upsert_microsoft_account(
            &path,
            "550e8400-e29b-41d4-a716-446655440000",
            "NewName",
            Some(200.0),
        )
        .unwrap();

        assert_eq!(updated.id, "ms-original", "must reuse existing id");
        assert_eq!(updated.name, "NewName");
        assert_eq!(updated.expires_at, Some(200.0));

        let on_disk = read_account_file(&path).unwrap();
        assert_eq!(on_disk.accounts.len(), 1, "no duplicate entry");
    }

    #[test]
    fn prepare_refresh_by_id_updates_existing_and_does_not_persist() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("account.json");
        let existing = Account {
            id: "ms-keep".into(),
            kind: AccountKind::Microsoft,
            name: "OldName".into(),
            uuid: "550e8400-e29b-41d4-a716-446655440000".into(),
            expires_at: Some(100.0),
        };
        write_account_file(
            &path,
            &AccountFile {
                version: 3,
                accounts: vec![existing.clone()],
                active_id: Some("ms-keep".into()),
            },
        )
        .unwrap();

        let (file, updated) = prepare_refresh_microsoft_account_by_id(
            &path,
            "ms-keep",
            "550e8400-e29b-41d4-a716-446655440000",
            "NewName",
            Some(200.0),
        )
        .unwrap();
        assert_eq!(updated.id, "ms-keep");
        assert_eq!(updated.name, "NewName");
        assert_eq!(updated.expires_at, Some(200.0));

        // Not persisted yet — on-disk still shows the old name.
        let on_disk = read_account_file(&path).unwrap();
        assert_eq!(on_disk.accounts[0].name, "OldName");

        // Persisting the returned file applies the update.
        write_account_file(&path, &file).unwrap();
        let after = read_account_file(&path).unwrap();
        assert_eq!(after.accounts[0].name, "NewName");
    }

    #[test]
    fn prepare_refresh_by_id_errors_when_account_removed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("account.json");
        write_account_file(&path, &AccountFile::default()).unwrap();

        let result = prepare_refresh_microsoft_account_by_id(
            &path,
            "ms-gone",
            "550e8400-e29b-41d4-a716-446655440000",
            "Ghost",
            Some(200.0),
        );
        assert!(
            matches!(result, Err(Error::AuthFailed { ref stage, .. }) if stage == "refresh"),
            "expected AuthFailed(refresh), got {result:?}"
        );
        // The store must not have gained a resurrected entry.
        let on_disk = read_account_file(&path).unwrap();
        assert!(on_disk.accounts.is_empty());
    }
}

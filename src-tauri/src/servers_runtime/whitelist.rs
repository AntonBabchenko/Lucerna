//! Structured editor for a server's `whitelist.json` and `ops.json` (#9, C4).
//!
//! Pure file CRUD over the two JSON arrays Minecraft reads from the server
//! working directory (`runtime/`). UUID resolution — online via Mojang or
//! offline derivation — lives in the command layer (it needs network + the
//! server's `online-mode`); this module just round-trips fully-formed entries,
//! so it is unit-testable without a network or an `AppHandle`.
//!
//! The "lockout fix" (toggling `white-list=true` must be paired with adding a
//! player) is enforced at the UI layer (S3): the editor surfaces an add path
//! alongside the toggle. This module provides that add path.

use crate::error::{Error, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

/// One `whitelist.json` entry. Minecraft matches whitelisted players by UUID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct WhitelistEntry {
    pub uuid: String,
    pub name: String,
}

/// One `ops.json` entry. `level` 4 = full operator; `bypassesPlayerLimit`
/// lets the op join past `max-players`. The on-disk key is camelCase
/// (`bypassesPlayerLimit`) — serde rename keeps disk, IPC, and the generated
/// TS binding in agreement (specta honours serde renames in this project).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct OpEntry {
    pub uuid: String,
    pub name: String,
    #[serde(default = "default_op_level")]
    pub level: u8,
    #[serde(rename = "bypassesPlayerLimit", default)]
    pub bypasses_player_limit: bool,
}

fn default_op_level() -> u8 {
    4
}

fn whitelist_path(runtime: &Path) -> PathBuf {
    runtime.join("whitelist.json")
}

fn ops_path(runtime: &Path) -> PathBuf {
    runtime.join("ops.json")
}

/// Read a JSON array file. Absent or empty → empty list (a fresh server has no
/// whitelist/ops files yet). A present-but-unparseable file is a hard error —
/// silently discarding a corrupt list could mass-deop players on the next write.
fn read_array<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(Vec::new()),
        Ok(s) => serde_json::from_str(&s)
            .map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(Error::io(path.display().to_string(), e)),
    }
}

/// Write a JSON array file (pretty-printed, matching Minecraft's own style),
/// creating the parent `runtime/` dir if needed.
fn write_array<T: Serialize>(path: &Path, items: &[T]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir.display().to_string(), e))?;
    }
    let json = serde_json::to_string_pretty(items)
        .map_err(|e| Error::io(path.display().to_string(), format!("serialize: {e}")))?;
    std::fs::write(path, json).map_err(|e| Error::io(path.display().to_string(), e))
}

/// True iff two identities refer to the same player. The UUID is the real
/// identity: when both sides have one, compare ONLY UUIDs. Offline UUIDs are
/// derived from the exact (case-sensitive) name, so "Steve" and "steve" are
/// genuinely different offline players and must not collapse — hence the name
/// fallback (used only when a UUID is missing) is case-SENSITIVE.
fn same_player(uuid_a: &str, name_a: &str, uuid_b: &str, name_b: &str) -> bool {
    match (uuid_a.is_empty(), uuid_b.is_empty()) {
        (false, false) => uuid_a.eq_ignore_ascii_case(uuid_b),
        _ => name_a == name_b,
    }
}

/// True iff `entry`'s identity matches `key` (a UUID or a player name).
fn key_matches(uuid: &str, name: &str, key: &str) -> bool {
    uuid.eq_ignore_ascii_case(key) || name.eq_ignore_ascii_case(key)
}

// ── whitelist.json ───────────────────────────────────────────────────────────

pub fn list_whitelist(runtime: &Path) -> Result<Vec<WhitelistEntry>> {
    read_array(&whitelist_path(runtime))
}

/// Add (or replace, if the same player is already present) a whitelist entry.
/// Returns the updated list.
pub fn add_whitelist(runtime: &Path, entry: WhitelistEntry) -> Result<Vec<WhitelistEntry>> {
    let mut list = list_whitelist(runtime)?;
    list.retain(|e| !same_player(&e.uuid, &e.name, &entry.uuid, &entry.name));
    list.push(entry);
    write_array(&whitelist_path(runtime), &list)?;
    Ok(list)
}

/// Remove a whitelist entry by UUID or name (case-insensitive). Idempotent.
pub fn remove_whitelist(runtime: &Path, key: &str) -> Result<Vec<WhitelistEntry>> {
    let mut list = list_whitelist(runtime)?;
    list.retain(|e| !key_matches(&e.uuid, &e.name, key));
    write_array(&whitelist_path(runtime), &list)?;
    Ok(list)
}

// ── ops.json ─────────────────────────────────────────────────────────────────

pub fn list_ops(runtime: &Path) -> Result<Vec<OpEntry>> {
    read_array(&ops_path(runtime))
}

/// Add (or replace) an operator entry. Returns the updated list.
pub fn add_op(runtime: &Path, entry: OpEntry) -> Result<Vec<OpEntry>> {
    let mut list = list_ops(runtime)?;
    list.retain(|e| !same_player(&e.uuid, &e.name, &entry.uuid, &entry.name));
    list.push(entry);
    write_array(&ops_path(runtime), &list)?;
    Ok(list)
}

/// Remove an operator entry by UUID or name (case-insensitive). Idempotent.
pub fn remove_op(runtime: &Path, key: &str) -> Result<Vec<OpEntry>> {
    let mut list = list_ops(runtime)?;
    list.retain(|e| !key_matches(&e.uuid, &e.name, key));
    write_array(&ops_path(runtime), &list)?;
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn entry(uuid: &str, name: &str) -> WhitelistEntry {
        WhitelistEntry {
            uuid: uuid.into(),
            name: name.into(),
        }
    }

    #[test]
    fn list_is_empty_when_file_absent() {
        let d = tempdir().unwrap();
        assert!(list_whitelist(d.path()).unwrap().is_empty());
        assert!(list_ops(d.path()).unwrap().is_empty());
    }

    #[test]
    fn whitelist_add_list_remove_roundtrip() {
        let d = tempdir().unwrap();
        add_whitelist(
            d.path(),
            entry("11111111-1111-1111-1111-111111111111", "Alice"),
        )
        .unwrap();
        add_whitelist(
            d.path(),
            entry("22222222-2222-2222-2222-222222222222", "Bob"),
        )
        .unwrap();
        let list = list_whitelist(d.path()).unwrap();
        assert_eq!(list.len(), 2);
        // Persisted in Minecraft's shape: an array of {uuid,name}.
        let raw = std::fs::read_to_string(d.path().join("whitelist.json")).unwrap();
        assert!(raw.contains("\"uuid\""));
        assert!(raw.contains("\"Alice\""));

        let after = remove_whitelist(d.path(), "Alice").unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].name, "Bob");
    }

    #[test]
    fn whitelist_add_dedupes_same_name_replacing_uuid() {
        let d = tempdir().unwrap();
        // A hand-edited entry with no UUID, then a re-add of the same name with a
        // now-resolved UUID → replaces, not duplicates (name fallback when a UUID
        // is missing).
        add_whitelist(d.path(), entry("", "Alice")).unwrap();
        let list = add_whitelist(
            d.path(),
            entry("11111111-1111-1111-1111-111111111111", "Alice"),
        )
        .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].uuid, "11111111-1111-1111-1111-111111111111");
    }

    #[test]
    fn whitelist_dedupes_by_uuid_not_name_case() {
        let d = tempdir().unwrap();
        // Two offline players whose names differ only by case derive DIFFERENT
        // UUIDs — they are distinct players and must both survive.
        add_whitelist(d.path(), entry("uuid-steve", "Steve")).unwrap();
        let list = add_whitelist(d.path(), entry("uuid-steve-lower", "steve")).unwrap();
        assert_eq!(
            list.len(),
            2,
            "case-distinct offline players are not merged"
        );
        // Re-adding the SAME uuid (different name casing) replaces by identity.
        let list = add_whitelist(d.path(), entry("UUID-STEVE", "Steverino")).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|e| e.name == "Steverino"));
    }

    #[test]
    fn remove_by_uuid_or_name_is_idempotent() {
        let d = tempdir().unwrap();
        add_whitelist(d.path(), entry("abc", "Carol")).unwrap();
        remove_whitelist(d.path(), "abc").unwrap(); // by uuid
        assert!(list_whitelist(d.path()).unwrap().is_empty());
        // Removing again is a no-op (still Ok).
        remove_whitelist(d.path(), "Carol").unwrap();
    }

    #[test]
    fn ops_roundtrip_writes_camelcase_key_and_default_level() {
        let d = tempdir().unwrap();
        add_op(
            d.path(),
            OpEntry {
                uuid: "33333333-3333-3333-3333-333333333333".into(),
                name: "Dave".into(),
                level: 4,
                bypasses_player_limit: false,
            },
        )
        .unwrap();
        let raw = std::fs::read_to_string(d.path().join("ops.json")).unwrap();
        assert!(raw.contains("\"bypassesPlayerLimit\""), "got: {raw}");
        assert!(raw.contains("\"level\": 4"));
        let back = list_ops(d.path()).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "Dave");
    }

    #[test]
    fn op_default_level_when_missing_in_file() {
        let d = tempdir().unwrap();
        // A hand-written ops.json omitting level/bypassesPlayerLimit.
        std::fs::write(d.path().join("ops.json"), r#"[{"uuid":"u","name":"Eve"}]"#).unwrap();
        let ops = list_ops(d.path()).unwrap();
        assert_eq!(ops[0].level, 4);
        assert!(!ops[0].bypasses_player_limit);
    }

    #[test]
    fn corrupt_file_is_a_hard_error_not_silent_reset() {
        let d = tempdir().unwrap();
        std::fs::write(d.path().join("whitelist.json"), b"{ not an array").unwrap();
        assert!(list_whitelist(d.path()).is_err());
    }
}

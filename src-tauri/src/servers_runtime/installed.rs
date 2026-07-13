//! Per-server installed-mods/plugins sidecar registry.
//!
//! Mirrors `crate::mods::installed` (the client per-instance registry) for a
//! server's jar directory (`runtime/mods/` for mod loaders, `runtime/plugins/`
//! for Paper/Purpur cores). Deltas from the client registry:
//!   * No `enabled` field on the record — activation is the on-disk `.jar` vs
//!     `.jar.disabled` suffix, resolved at list time and returned as
//!     `ServerInstalledEntry::enabled`.
//!   * No `requires` edge list — server orphan detection is out of scope.
//!   * Synchronous `std::fs`, matching the sibling `servers_runtime` modules
//!     (`plugins.rs`, `quarantine.rs`, `store.rs`).
//!
//! Keyed by sha1 (lowercased) so identity survives an enable/disable rename.
//! Fail-open: a missing/corrupt sidecar is treated as empty and rebuilt from
//! disk by `reconcile_on_list` — never a hard error.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::error::{Error, Result};
use crate::mods::platform::ModSource;

const FILE_VERSION: u32 = 1;
const SIDECAR: &str = ".lucerna-installed.json";

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct ServerInstalledRecord {
    pub filename: String,
    pub sha1: String,
    pub source: Option<ModSource>,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub name: Option<String>,
    pub version_number: Option<String>,
    #[serde(default)]
    pub enrich_attempted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct ServerInstalledEntry {
    pub record: ServerInstalledRecord,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Sidecar {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    records: Vec<ServerInstalledRecord>,
}

fn default_version() -> u32 {
    FILE_VERSION
}

fn sidecar_path(jar_dir: &Path) -> PathBuf {
    jar_dir.join(SIDECAR)
}

pub fn load(jar_dir: &Path) -> Vec<ServerInstalledRecord> {
    let Ok(bytes) = std::fs::read(sidecar_path(jar_dir)) else {
        return Vec::new();
    };
    serde_json::from_slice::<Sidecar>(&bytes)
        .map(|s| s.records)
        .unwrap_or_default()
}

pub fn save(jar_dir: &Path, records: &[ServerInstalledRecord]) -> Result<()> {
    std::fs::create_dir_all(jar_dir).map_err(|e| Error::io(jar_dir.display().to_string(), e))?;
    let final_path = sidecar_path(jar_dir);
    let tmp = final_path.with_extension(format!("json.tmp.{}", std::process::id()));
    let sidecar = Sidecar {
        version: FILE_VERSION,
        records: records.to_vec(),
    };
    let bytes = serde_json::to_vec_pretty(&sidecar)
        .map_err(|e| Error::io(final_path.display().to_string(), e))?;
    std::fs::write(&tmp, &bytes).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, &final_path).map_err(|e| Error::io(final_path.display().to_string(), e))
}

pub fn sha1_of(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path.display().to_string(), e))?;
    Ok(hex::encode(Sha1::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(filename: &str, sha: &str) -> ServerInstalledRecord {
        ServerInstalledRecord {
            filename: filename.into(),
            sha1: sha.into(),
            source: Some(ModSource::Modrinth),
            project_id: Some("proj".into()),
            version_id: Some("ver".into()),
            name: Some("Nice Mod".into()),
            version_number: Some("1.2.3".into()),
            enrich_attempted: true,
        }
    }

    #[test]
    fn load_is_fail_open_on_missing_and_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_empty());
        std::fs::write(sidecar_path(dir.path()), b"{ not json").unwrap();
        assert!(load(dir.path()).is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let records = vec![rec("a.jar", "aa"), rec("b.jar", "bb")];
        save(dir.path(), &records).unwrap();
        assert_eq!(load(dir.path()), records);
    }
}

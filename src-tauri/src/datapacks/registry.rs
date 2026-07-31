//! Per-instance installed-datapacks registry.
//!
//! File: `{instance}/lucerna/installed-datapacks.json`. Schema v1.
//!
//! On every read the registry is reconciled against the real contents of
//! `{instance}/datapacks/`, so hand-dropped and hand-deleted files settle
//! cleanly — the same supported workflow `installed-mods.json` has.
//!
//! There is deliberately no `enabled` field — one library entry fans out to N
//! worlds, each with its own state in its own level.dat, so a scalar would have
//! no well-defined value. Enabled state is read from level.dat on demand.
//!
//! Writes go through `mods::store::place_bytes`, which already gives every
//! write a unique temp name (its own counter) and an atomic rename onto the
//! final path — unlike `mods::installed`'s `write()`, this module needs no
//! `WRITE_SEQ` of its own.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::datapacks::{library_dir_at, registry_path_at, InstalledDatapack};
use crate::error::{Error, Result};

const FILE_VERSION: u32 = 1;

// Deliberately no `#[derive(Default)]`: a default `version: u32` would be `0`,
// and calling `.unwrap_or_default()` anywhere would reintroduce exactly the
// bug `read_or_empty`'s doc comment above warns against. Every construction
// site below sets `version: FILE_VERSION` explicitly.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OnDisk {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    datapacks: Vec<InstalledDatapack>,
}

fn default_version() -> u32 {
    FILE_VERSION
}

fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::ModsInstancePath {
        path: path.display().to_string(),
        details: e.to_string(),
    }
}

/// Read the registry from disk, or a fresh current-version state when the
/// file is absent or unreadable-as-JSON. Constructing `version: FILE_VERSION`
/// explicitly here (rather than `OnDisk::default()`, which would leave
/// `version` at `u32`'s own zero default) means a genuinely fresh instance
/// takes the "already current" branch of `migrate()` and needs no write —
/// `list()` on an instance that has never touched datapacks creates no
/// `lucerna/` directory at all. Mirrors `mods::installed::read_or_empty`.
async fn read_or_empty(instance_root: &Path) -> OnDisk {
    let path = registry_path_at(instance_root);
    let Ok(bytes) = fs::read(&path).await else {
        return OnDisk {
            version: FILE_VERSION,
            datapacks: vec![],
        };
    };
    // A corrupt registry is metadata loss, never content loss — reconcile will
    // re-adopt every file in the library dir.
    serde_json::from_slice::<OnDisk>(&bytes).unwrap_or(OnDisk {
        version: FILE_VERSION,
        datapacks: vec![],
    })
}

async fn write(instance_root: &Path, state: &OnDisk) -> Result<()> {
    let final_path = registry_path_at(instance_root);
    // `registry_path_at` always joins `<instance_root>/lucerna/installed-datapacks.json`,
    // which always has at least two components — `parent()` can never be `None`.
    let dir = final_path
        .parent()
        .expect("registry_path_at always returns a path with a parent directory");
    fs::create_dir_all(dir).await.map_err(|e| io_err(dir, e))?;

    let bytes = serde_json::to_vec_pretty(state).map_err(|e| Error::ModsDecode {
        platform: "installed-datapacks.json".into(),
        details: e.to_string(),
    })?;
    crate::mods::store::place_bytes(&final_path, &bytes)
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: e.path.display().to_string(),
            details: e.details(),
        })
}

/// Drop entries whose file is gone; adopt `.zip` files that have no entry.
/// Returns true when anything changed and the caller should persist.
async fn reconcile(instance_root: &Path, state: &mut OnDisk) -> bool {
    let lib = library_dir_at(instance_root);
    let mut on_disk: Vec<String> = Vec::new();
    if let Ok(mut rd) = fs::read_dir(&lib).await {
        while let Ok(Some(e)) = rd.next_entry().await {
            let name = e.file_name().to_string_lossy().to_string();
            if name.to_ascii_lowercase().ends_with(".zip") {
                on_disk.push(name);
            }
        }
    }

    let before = state.datapacks.len();
    state.datapacks.retain(|d| on_disk.contains(&d.filename));
    let mut changed = state.datapacks.len() != before;

    for name in on_disk {
        if state.datapacks.iter().any(|d| d.filename == name) {
            continue;
        }
        let path = lib.join(&name);
        let bytes = fs::read(&path).await.unwrap_or_default();
        let meta = crate::datapacks::pack_meta::read_meta(&bytes);
        state.datapacks.push(InstalledDatapack {
            name: meta
                .description
                .unwrap_or_else(|| name.trim_end_matches(".zip").to_string()),
            pack_format: meta.pack_format,
            size_bytes: bytes.len() as f64,
            sha1: crate::datapacks::library::sha1_hex(&bytes),
            filename: name,
            source: None,
            project_id: None,
            version_id: None,
            installed_at: chrono::Utc::now().to_rfc3339(),
        });
        changed = true;
    }
    changed
}

/// Read the registry, reconciled against `{instance}/datapacks/`. Persists the
/// reconciled state (and any schema migration) back to disk when either
/// changed anything.
pub async fn list(instance_root: &Path) -> Result<Vec<InstalledDatapack>> {
    let mut state = read_or_empty(instance_root).await;
    let migrated = migrate(&mut state);
    let reconciled = reconcile(instance_root, &mut state).await;
    if migrated || reconciled {
        write(instance_root, &state).await?;
    }
    let mut out = state.datapacks;
    out.sort_by(|a, b| a.filename.to_lowercase().cmp(&b.filename.to_lowercase()));
    Ok(out)
}

/// Append a new entry, replacing any existing entry with the same filename.
/// Caller has already placed the file in `{instance}/datapacks/`.
pub async fn add(instance_root: &Path, item: InstalledDatapack) -> Result<()> {
    let mut state = read_or_empty(instance_root).await;
    state.datapacks.retain(|d| d.filename != item.filename);
    state.datapacks.push(item);
    state.version = FILE_VERSION;
    write(instance_root, &state).await
}

/// Remove the entry with the given filename. Caller is responsible for
/// removing the physical file, if that is also wanted.
pub async fn remove(instance_root: &Path, filename: &str) -> Result<()> {
    let mut state = read_or_empty(instance_root).await;
    state.datapacks.retain(|d| d.filename != filename);
    state.version = FILE_VERSION;
    write(instance_root, &state).await
}

/// No migrations exist yet; the ladder is here so the first schema change is a
/// three-line edit rather than a redesign.
fn migrate(state: &mut OnDisk) -> bool {
    if state.version >= FILE_VERSION {
        return false;
    }
    state.version = FILE_VERSION;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(filename: &str, sha1: &str) -> InstalledDatapack {
        InstalledDatapack {
            filename: filename.into(),
            sha1: sha1.into(),
            size_bytes: 10.0,
            pack_format: Some(48),
            name: filename.trim_end_matches(".zip").into(),
            source: None,
            project_id: None,
            version_id: None,
            installed_at: "2026-07-31T00:00:00Z".into(),
        }
    }

    #[tokio::test]
    async fn list_is_empty_for_a_fresh_instance() {
        let td = tempfile::tempdir().unwrap();
        assert!(list(td.path()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn add_then_list_round_trips_through_disk() {
        let td = tempfile::tempdir().unwrap();
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("vm.zip"), b"PACK").unwrap();

        add(td.path(), entry("vm.zip", "aaa")).await.unwrap();
        let got = list(td.path()).await.unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].filename, "vm.zip");
    }

    #[tokio::test]
    async fn an_entry_whose_file_vanished_is_dropped_on_read() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(crate::datapacks::library_dir_at(td.path())).unwrap();
        add(td.path(), entry("gone.zip", "bbb")).await.unwrap();
        assert!(list(td.path()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_hand_dropped_file_is_adopted_with_no_provenance() {
        let td = tempfile::tempdir().unwrap();
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("manual.zip"), b"PACK").unwrap();

        let got = list(td.path()).await.unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].filename, "manual.zip");
        assert_eq!(got[0].source, None);
    }

    #[tokio::test]
    async fn only_zip_files_are_adopted() {
        let td = tempfile::tempdir().unwrap();
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("notes.txt"), b"hi").unwrap();
        assert!(list(td.path()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn remove_drops_the_entry_but_not_the_file() {
        let td = tempfile::tempdir().unwrap();
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("vm.zip"), b"PACK").unwrap();
        add(td.path(), entry("vm.zip", "aaa")).await.unwrap();

        remove(td.path(), "vm.zip").await.unwrap();
        let raw = std::fs::read_to_string(crate::datapacks::registry_path_at(td.path())).unwrap();
        assert!(!raw.contains("\"vm.zip\""));
        assert!(lib.join("vm.zip").exists());
    }

    #[tokio::test]
    async fn the_version_key_is_written_from_day_one() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(crate::datapacks::library_dir_at(td.path())).unwrap();
        add(td.path(), entry("vm.zip", "aaa")).await.unwrap();
        let raw = std::fs::read_to_string(crate::datapacks::registry_path_at(td.path())).unwrap();
        // A missing `version` key would default to CURRENT on read, so a file
        // written without it could never be migrated later.
        assert!(raw.contains("\"version\": 1"));
    }

    #[tokio::test]
    async fn a_corrupt_registry_reads_as_empty_rather_than_failing() {
        let td = tempfile::tempdir().unwrap();
        let p = crate::datapacks::registry_path_at(td.path());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"{ not json").unwrap();
        assert!(list(td.path()).await.unwrap().is_empty());
    }
}

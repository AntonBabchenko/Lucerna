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
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::datapacks::{library_dir_at, registry_path_at, InstalledDatapack};
use crate::error::{Error, Result};

const FILE_VERSION: u32 = 1;

/// Serializes every read-modify-write of the registry file: `list`'s
/// reconcile-then-persist and `add`/`remove`'s own read-modify-write can
/// otherwise interleave across the concurrent tasks Tauri runs each command
/// as, and whichever write lands last wins with the other's entry silently
/// dropped — the registry equivalent of `world_link`'s level.dat lost-update
/// bug. `reconcile` re-adopts a physically-present file on the next read, but
/// it cannot reconstruct `installed_at`, nor (once the catalog lands)
/// `source`/`project_id`/`version_id` — those are gone for good.
///
/// A SEPARATE mutex from `world_link::level_dat_lock`, not the same one:
/// nothing in this file ever calls into `world_link`, and the one place
/// `world_link` calls into this module — `list_for_world_at` calling
/// `registry::list` — does so without ever having taken `level_dat_lock`
/// first (only the three level.dat-mutating entry points take that lock, and
/// none of them calls `registry::*`). So the two locks are never both held by
/// the same call stack, and keeping them apart cannot deadlock.
/// `tokio::sync::Mutex`, not `std::sync::Mutex`: the critical section spans
/// the `.await` points in `read_or_empty`/`reconcile`/`write`.
fn registry_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

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

    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|e| Error::io(final_path.display().to_string(), e))?;
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
    match fs::read_dir(&lib).await {
        Ok(mut rd) => {
            while let Ok(Some(e)) = rd.next_entry().await {
                let name = e.file_name().to_string_lossy().to_string();
                if name.to_ascii_lowercase().ends_with(".zip") {
                    on_disk.push(name);
                }
            }
        }
        // A fresh instance with no datapacks/ dir yet: every entry really is
        // gone, so retaining against an empty on_disk list is correct.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        // Anything else — permission denied, a relocated data root caught
        // mid-move, a transient hiccup — is NOT proof the library is empty.
        // Treating it as one would `retain` every entry away and `list` would
        // persist that empty state, permanently losing every pack's
        // provenance for a problem that was never about the packs. Skip
        // reconciling entirely; report nothing changed so the caller does
        // not persist a wipe.
        Err(e) => {
            crate::diag!(
                "datapacks: registry reconcile skipped, could not read {}: {e}",
                lib.display()
            );
            return false;
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
        // A merely-unreadable file (permission change, AV hold, deleted
        // between the listing above and this read) must not be adopted as a
        // fabricated zero-byte entry — that presents a guess as fact (empty
        // sha1, no pack_format, size 0). Skip adoption this pass; the next
        // `list()` call tries again.
        let bytes = match fs::read(&path).await {
            Ok(b) => b,
            Err(e) => {
                crate::diag!(
                    "datapacks: skipping adoption of {}, unreadable: {e}",
                    path.display()
                );
                continue;
            }
        };
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
/// changed anything. The persist is best-effort: a full disk or a read-only
/// data root must not turn an otherwise-successful listing into an error page
/// — every pack still listed fine, only the housekeeping write failed.
pub async fn list(instance_root: &Path) -> Result<Vec<InstalledDatapack>> {
    let _guard = registry_lock().lock().await;
    let mut state = read_or_empty(instance_root).await;
    let migrated = migrate(&mut state);
    let reconciled = reconcile(instance_root, &mut state).await;
    if migrated || reconciled {
        if let Err(e) = write(instance_root, &state).await {
            crate::diag!(
                "datapacks: registry persist failed for {}: {e}",
                instance_root.display()
            );
        }
    }
    let mut out = state.datapacks;
    out.sort_by(|a, b| a.filename.to_lowercase().cmp(&b.filename.to_lowercase()));
    Ok(out)
}

/// Append a new entry, replacing any existing entry with the same filename.
/// Caller has already placed the file in `{instance}/datapacks/`.
pub async fn add(instance_root: &Path, item: InstalledDatapack) -> Result<()> {
    let _guard = registry_lock().lock().await;
    let mut state = read_or_empty(instance_root).await;
    state.datapacks.retain(|d| d.filename != item.filename);
    state.datapacks.push(item);
    state.version = FILE_VERSION;
    write(instance_root, &state).await
}

/// Remove the entry with the given filename. Caller is responsible for
/// removing the physical file, if that is also wanted.
pub async fn remove(instance_root: &Path, filename: &str) -> Result<()> {
    let _guard = registry_lock().lock().await;
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

    #[tokio::test]
    async fn a_non_notfound_read_dir_error_does_not_wipe_the_registry() {
        let td = tempfile::tempdir().unwrap();
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("vm.zip"), b"PACK").unwrap();
        add(td.path(), entry("vm.zip", "aaa")).await.unwrap();

        // Replace the library DIRECTORY with a plain FILE. `read_dir` against
        // a file fails with something other than `NotFound` on every
        // platform — the same shape as a permission error or a relocated
        // data root caught mid-move, and NOT the "fresh instance" case.
        std::fs::remove_dir_all(&lib).unwrap();
        std::fs::write(&lib, b"not a directory").unwrap();

        let got = list(td.path()).await.unwrap();
        assert_eq!(
            got.len(),
            1,
            "a transient (non-NotFound) read_dir failure must not wipe existing entries"
        );
        assert_eq!(got[0].filename, "vm.zip");
    }

    #[tokio::test]
    async fn an_unreadable_pack_is_not_adopted_as_a_fabricated_zero_byte_entry() {
        let td = tempfile::tempdir().unwrap();
        let lib = crate::datapacks::library_dir_at(td.path());
        // A DIRECTORY named "sneaky.zip" passes the on-disk `.zip` name
        // filter, but `fs::read` on a directory fails — reconcile must skip
        // adopting it rather than recording `size_bytes: 0` and an empty
        // sha1 as if that were a real (if empty) pack.
        std::fs::create_dir_all(lib.join("sneaky.zip")).unwrap();

        let got = list(td.path()).await.unwrap();
        assert!(
            got.is_empty(),
            "an unreadable entry must not be adopted with fabricated metadata"
        );
    }

    #[tokio::test]
    async fn a_persist_failure_does_not_turn_list_into_an_error() {
        let td = tempfile::tempdir().unwrap();
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("vm.zip"), b"PACK").unwrap();

        // Occupy the `lucerna/` directory's own path with a plain file, so
        // the persist that adopting "vm.zip" would trigger fails inside
        // `write`'s `create_dir_all` — the same shape as a full disk or a
        // read-only data root. `list` must still return the reconciled data.
        let lucerna_dir = td.path().join("lucerna");
        std::fs::write(&lucerna_dir, b"occupied").unwrap();

        let got = list(td.path()).await.unwrap();
        assert_eq!(got.len(), 1, "reconciled data must still be returned");
        assert_eq!(got[0].filename, "vm.zip");
    }

    /// Regression for the registry's lost-update window: `add`'s
    /// read-modify-write on `installed-datapacks.json` used to have no mutual
    /// exclusion, so two concurrent `add` calls could both read the same
    /// on-disk state, and whichever wrote last would silently drop the
    /// other's entry. With `registry_lock` serializing every call, this is
    /// deterministic regardless of scheduling.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_adds_do_not_lose_an_entry() {
        let td = tempfile::tempdir().unwrap();
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        for i in 0..8 {
            std::fs::write(lib.join(format!("p{i}.zip")), b"PACK").unwrap();
        }

        let mut handles = Vec::new();
        for i in 0..8 {
            let root = td.path().to_path_buf();
            handles.push(tokio::spawn(async move {
                add(&root, entry(&format!("p{i}.zip"), "sha")).await
            }));
        }
        for h in handles {
            h.await.unwrap().unwrap();
        }

        let got = list(td.path()).await.unwrap();
        assert_eq!(got.len(), 8, "every concurrent add must survive");
    }
}

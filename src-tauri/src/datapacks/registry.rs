//! Per-instance installed-datapacks registry.
//!
//! File: `{instance}/lucerna/installed-datapacks.json`. Schema v3 —
//! v1 → v2 added `version_number`; v2 → v3 added each row's `mcmeta`; see
//! [`migrate`].
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

use crate::datapacks::format::PackMcmeta;
use crate::datapacks::pack_meta;
use crate::datapacks::{library_dir_at, registry_path_at, InstalledDatapack};
use crate::error::{Error, Result};

const FILE_VERSION: u32 = 3;

/// One registry row as stored: the pack the UI sees, plus what its
/// `pack.mcmeta` declares (§1 C4). The declaration stays off IPC — the UI
/// gets the verdict, which depends on the instance's version and so is never
/// stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredRow {
    #[serde(flatten)]
    pub pack: InstalledDatapack,
    /// `None` ⟺ this build has not read the file's declaration yet: a v2
    /// row, a value a newer build wrote in a shape this build cannot parse,
    /// or a read that failed on I/O. `reconcile` backfills it from the
    /// library file and retries on every listing until it has an answer —
    /// "could not tell" is never stamped as a fact.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient_mcmeta"
    )]
    pub mcmeta: Option<PackMcmeta>,
}

/// Reads `mcmeta` as an untyped [`serde_json::Value`] FIRST, then tries the
/// typed shape. A value this build cannot parse (a newer build's variant, a
/// downgrade round-trip) becomes `None` — re-read from the file, the source
/// of truth — instead of failing the WHOLE registry parse, which
/// `read_or_empty` turns into an empty registry with every row's provenance
/// gone (§0.5 A23). Never call `PackMcmeta::deserialize` on the outer
/// deserializer: an error there leaves it half-consumed.
fn lenient_mcmeta<'de, D>(d: D) -> std::result::Result<Option<PackMcmeta>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(d)?;
    Ok(raw.and_then(|v| serde_json::from_value::<PackMcmeta>(v).ok()))
}

/// The recorded declaration of the row named exactly `filename`.
#[must_use]
pub fn mcmeta_of<'a>(rows: &'a [StoredRow], filename: &str) -> Option<&'a PackMcmeta> {
    rows.iter()
        .find(|r| r.pack.filename == filename)
        .and_then(|r| r.mcmeta.as_ref())
}

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
    datapacks: Vec<StoredRow>,
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
        Ok(mut rd) => loop {
            let e = match rd.next_entry().await {
                Ok(Some(e)) => e,
                Ok(None) => break,
                // An entry that cannot be read is not proof its file is gone:
                // retaining against a partial list would drop that pack's row
                // and its provenance for good. Skip reconciling, as below.
                Err(e) => {
                    crate::diag!(
                        "datapacks: registry reconcile skipped, could not read an entry of {}: {e}",
                        lib.display()
                    );
                    return false;
                }
            };
            let name = e.file_name().to_string_lossy().to_string();
            // Case-folded on purpose (spec §2 N.3): the game never scans the
            // library dir, and N.4/N.5 enforce exactness where a file enters a
            // world.
            if name.to_ascii_lowercase().ends_with(".zip") {
                on_disk.push(name);
            }
        },
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
    state
        .datapacks
        .retain(|d| on_disk.contains(&d.pack.filename));
    let mut changed = state.datapacks.len() != before;

    for name in on_disk {
        if state.datapacks.iter().any(|d| d.pack.filename == name) {
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
        let meta = pack_meta::read_meta(&bytes);
        let pack = InstalledDatapack {
            name: meta
                .description
                .unwrap_or_else(|| name.trim_end_matches(".zip").to_string()),
            size_bytes: bytes.len() as f64,
            sha1: crate::datapacks::library::sha1_hex(&bytes),
            filename: name,
            source: None,
            project_id: None,
            version_id: None,
            version_number: None,
            installed_at: chrono::Utc::now().to_rfc3339(),
        };
        state.datapacks.push(StoredRow {
            pack,
            mcmeta: Some(meta.mcmeta),
        });
        changed = true;
    }
    let backfilled = backfill(&lib, state).await;
    changed || backfilled
}

/// Fill `mcmeta` for every row that has none, from its library file (only
/// the central directory and `pack.mcmeta` are read, off the executor). A
/// parse outcome — `Missing`, `Unreadable`, a declaration — is a real answer
/// and is persisted; an I/O failure is "could not tell": the row stays
/// `None`, a `diag!` names it, and the next listing tries again. The display
/// name is re-derived from the same read: it is display-only and always
/// comes from the file.
async fn backfill(lib: &Path, state: &mut OnDisk) -> bool {
    let todo: Vec<(usize, std::path::PathBuf)> = state
        .datapacks
        .iter()
        .enumerate()
        .filter(|(_, r)| r.mcmeta.is_none())
        .map(|(i, r)| (i, lib.join(&r.pack.filename)))
        .collect();
    if todo.is_empty() {
        return false;
    }
    let paths: Vec<std::path::PathBuf> = todo.iter().map(|(_, p)| p.clone()).collect();
    let reads = match tokio::task::spawn_blocking(move || {
        paths
            .iter()
            .map(|p| pack_meta::read_meta_file(p))
            .collect::<Vec<_>>()
    })
    .await
    {
        Ok(reads) => reads,
        Err(e) => {
            crate::diag!(
                "datapacks: pack.mcmeta backfill did not run, retried on the next listing: {e}"
            );
            return false;
        }
    };
    let mut changed = false;
    for ((i, path), read) in todo.into_iter().zip(reads) {
        match read {
            Ok(meta) => {
                let row = &mut state.datapacks[i];
                let stem = row.pack.filename.trim_end_matches(".zip").to_string();
                row.pack.name = meta.description.unwrap_or(stem);
                row.mcmeta = Some(meta.mcmeta);
                changed = true;
            }
            Err(e) => crate::diag!(
                "datapacks: could not read pack.mcmeta of {}, retried on the next listing: {e}",
                path.display()
            ),
        }
    }
    changed
}

/// Read, reconcile and (best-effort) persist — the shared body of [`list`]
/// and [`list_rows`]. Sorted case-insensitively by filename. Persists the
/// reconciled state (and any schema migration) back to disk when either
/// changed anything. The persist is best-effort: a full disk or a read-only
/// data root must not turn an otherwise-successful listing into an error page
/// — every pack still listed fine, only the housekeeping write failed.
async fn reconciled_rows(instance_root: &Path) -> Vec<StoredRow> {
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
    out.sort_by(|a, b| {
        a.pack
            .filename
            .to_lowercase()
            .cmp(&b.pack.filename.to_lowercase())
    });
    out
}

/// The library's packs, reconciled against `{instance}/datapacks/`. The
/// persist is best-effort (see [`reconciled_rows`]).
pub async fn list(instance_root: &Path) -> Result<Vec<InstalledDatapack>> {
    Ok(reconciled_rows(instance_root)
        .await
        .into_iter()
        .map(|r| r.pack)
        .collect())
}

/// [`list`], with each row's recorded `pack.mcmeta` declaration — for the two
/// listings that compute a verdict (`overview`, `world_link::listing`).
pub async fn list_rows(instance_root: &Path) -> Result<Vec<StoredRow>> {
    Ok(reconciled_rows(instance_root).await)
}

/// Append a new entry, replacing any existing entry with the same filename.
/// `mcmeta` is what the same bytes' `pack.mcmeta` declares, computed by the
/// caller from the bytes it placed. Caller has already placed the file in
/// `{instance}/datapacks/`.
pub async fn add(instance_root: &Path, item: InstalledDatapack, mcmeta: PackMcmeta) -> Result<()> {
    let _guard = registry_lock().lock().await;
    let mut state = read_or_empty(instance_root).await;
    state.datapacks.retain(|d| d.pack.filename != item.filename);
    state.datapacks.push(StoredRow {
        pack: item,
        mcmeta: Some(mcmeta),
    });
    state.version = FILE_VERSION;
    write(instance_root, &state).await
}

/// Remove the entry with the given filename. Caller is responsible for
/// removing the physical file, if that is also wanted.
pub async fn remove(instance_root: &Path, filename: &str) -> Result<()> {
    let _guard = registry_lock().lock().await;
    let mut state = read_or_empty(instance_root).await;
    state.datapacks.retain(|d| d.pack.filename != filename);
    state.version = FILE_VERSION;
    write(instance_root, &state).await
}

/// v2 → v3: rows gained `mcmeta`. The backfill in `reconcile` is keyed on the
/// FIELD (`mcmeta: None`), never on this stamp, so a row whose read failed is
/// never stamped and forgotten.
///
/// v1 → v2: `version_number` was added to `InstalledDatapack` with
/// `#[serde(default)]`. No field backfill happens — or could: by the time this
/// runs, `read_or_empty` has already deserialized into the new shape and serde
/// has supplied the `None`. Bumping the version only STAMPS the upgrade, the
/// same shape `mods::installed`'s v3 → v4 uses.
///
/// Anything that genuinely needs to rewrite rows goes here as a `match` on the
/// old version before the bump.
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
    use crate::datapacks::format::{samples, Bound, Fact, FormatVersion, PackMcmeta};
    use std::path::Path;

    const TERRALITH: &str = r#"{"pack":{"min_format":107,"max_format":107,"description":[{"text":"Terralith","color":"green"},{"text":" by Stardust Labs"}]}}"#;

    fn seed_registry(root: &Path, json: &str) -> std::path::PathBuf {
        let lucerna = root.join("lucerna");
        std::fs::create_dir_all(&lucerna).unwrap();
        let reg = lucerna.join("installed-datapacks.json");
        std::fs::write(&reg, json).unwrap();
        reg
    }

    #[tokio::test]
    async fn a_v2_row_is_backfilled_from_its_library_file_and_keeps_provenance() {
        // A v2 row has no `mcmeta` and a stale `pack_format: null` (the corpus
        // Terralith row). The upgrade re-reads the library file: provenance
        // survives, the declaration is recorded, and the name comes from the
        // rich-text description instead of the filename stem.
        let td = tempfile::tempdir().unwrap();
        let reg = seed_registry(
            td.path(),
            r#"{"version":2,"datapacks":[{"filename":"terralith.zip","sha1":"abc","size_bytes":10.0,"pack_format":null,"name":"terralith","source":"modrinth","project_id":"p1","version_id":"v1","version_number":"2.5.8","installed_at":"2026-01-01T00:00:00Z"}]}"#,
        );
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(
            lib.join("terralith.zip"),
            samples::zip_with_mcmeta(TERRALITH),
        )
        .unwrap();

        let rows = list_rows(td.path()).await.unwrap();

        assert_eq!(rows.len(), 1, "the v2 row must survive");
        assert_eq!(rows[0].pack.project_id.as_deref(), Some("p1"));
        assert_eq!(rows[0].pack.version_id.as_deref(), Some("v1"));
        assert_eq!(rows[0].pack.name, "Terralith by Stardust Labs");
        match &rows[0].mcmeta {
            Some(PackMcmeta::Read(d)) => {
                assert_eq!(d.min_format, Fact::Present(Bound::Major(107)));
                assert_eq!(d.max_format, Fact::Present(Bound::Major(107)));
                assert_eq!(d.pack_format, Fact::Absent);
            }
            other => panic!("expected a recorded declaration, got {other:?}"),
        }
        let raw = std::fs::read_to_string(reg).unwrap();
        assert!(raw.contains("\"version\": 3"), "raw was: {raw}");
        assert!(
            raw.contains("\"mcmeta\""),
            "the backfill is persisted: {raw}"
        );
    }

    #[tokio::test]
    async fn an_unreadable_library_file_leaves_the_row_unread_and_intact() {
        // "Could not tell" is never stamped (Fallback discipline, q.2): an
        // entry that cannot be read — a DIRECTORY named like the zip — keeps
        // its row and provenance, records nothing (so its verdict is
        // Unknown), and is retried on the next listing.
        let td = tempfile::tempdir().unwrap();
        let reg = seed_registry(
            td.path(),
            r#"{"version":3,"datapacks":[{"filename":"x.zip","sha1":"abc","size_bytes":10.0,"name":"X","source":"modrinth","project_id":"p1","version_id":"v1","installed_at":"2026-01-01T00:00:00Z"}]}"#,
        );
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(lib.join("x.zip")).unwrap();

        let rows = list_rows(td.path()).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].pack.project_id.as_deref(), Some("p1"));
        assert_eq!(rows[0].mcmeta, None, "an I/O failure is not an answer");
        assert_eq!(
            crate::datapacks::verdict::verdict(
                rows[0].mcmeta.as_ref(),
                Some(FormatVersion::new(48, 0))
            ),
            crate::datapacks::PackCompat::Unknown
        );
        assert!(
            !std::fs::read_to_string(&reg)
                .unwrap()
                .contains("\"mcmeta\""),
            "nothing stamped"
        );

        std::fs::remove_dir_all(lib.join("x.zip")).unwrap();
        std::fs::write(lib.join("x.zip"), samples::zip_with_mcmeta(samples::DAGGER)).unwrap();
        let rows = list_rows(td.path()).await.unwrap();
        assert!(
            matches!(rows[0].mcmeta, Some(PackMcmeta::Read(_))),
            "retried: {:?}",
            rows[0].mcmeta
        );
        assert_eq!(rows[0].pack.project_id.as_deref(), Some("p1"));
    }

    #[tokio::test]
    async fn a_declaration_this_build_cannot_parse_does_not_wipe_the_registry() {
        // A newer build may store a variant this one does not know. A strict
        // parse fails the WHOLE file, `read_or_empty` returns an empty
        // registry, and every row's provenance is lost on the next write.
        let td = tempfile::tempdir().unwrap();
        seed_registry(
            td.path(),
            r#"{"version":3,"datapacks":[{"filename":"vm.zip","sha1":"abc","size_bytes":10.0,"name":"VM","source":"modrinth","project_id":"p1","version_id":"v1","installed_at":"2026-01-01T00:00:00Z","mcmeta":{"kind":"from_the_future","x":1}}]}"#,
        );
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(
            lib.join("vm.zip"),
            samples::zip_with_mcmeta(samples::DAGGER),
        )
        .unwrap();

        let rows = list_rows(td.path()).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].pack.project_id.as_deref(),
            Some("p1"),
            "provenance survives"
        );
        assert!(
            matches!(rows[0].mcmeta, Some(PackMcmeta::Read(_))),
            "re-read from the file"
        );
    }

    #[tokio::test]
    async fn a_parse_outcome_is_persisted_as_an_answer() {
        // The discrimination half: bytes that are not a zip ARE an answer
        // (Unreadable), unlike an I/O failure — recorded, not retried forever.
        let td = tempfile::tempdir().unwrap();
        let reg = seed_registry(
            td.path(),
            r#"{"version":3,"datapacks":[{"filename":"vm.zip","sha1":"abc","size_bytes":4.0,"name":"vm","source":null,"project_id":null,"version_id":null,"installed_at":"2026-01-01T00:00:00Z"}]}"#,
        );
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("vm.zip"), b"PACK").unwrap();

        let rows = list_rows(td.path()).await.unwrap();
        assert_eq!(rows[0].mcmeta, Some(PackMcmeta::Unreadable));
        assert!(std::fs::read_to_string(reg)
            .unwrap()
            .contains("\"unreadable\""));
    }

    #[tokio::test]
    async fn an_adopted_file_records_its_declaration_and_plain_name() {
        let td = tempfile::tempdir().unwrap();
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(
            lib.join("caps.zip"),
            samples::zip_with_mcmeta(samples::BETTERCAPS),
        )
        .unwrap();

        let rows = list_rows(td.path()).await.unwrap();
        assert_eq!(
            rows[0].pack.name,
            "Three-dimensional Caps! by _Lvnatic and RedRibbon!"
        );
        assert!(
            matches!(&rows[0].mcmeta, Some(PackMcmeta::Read(d)) if d.supported_formats == Fact::Present((34, 48)))
        );
    }

    fn entry(filename: &str, sha1: &str) -> InstalledDatapack {
        InstalledDatapack {
            filename: filename.into(),
            sha1: sha1.into(),
            size_bytes: 10.0,
            name: filename.trim_end_matches(".zip").into(),
            source: None,
            project_id: None,
            version_id: None,
            version_number: None,
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

        add(td.path(), entry("vm.zip", "aaa"), PackMcmeta::Unreadable)
            .await
            .unwrap();
        let got = list(td.path()).await.unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].filename, "vm.zip");
    }

    #[tokio::test]
    async fn an_entry_whose_file_vanished_is_dropped_on_read() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(crate::datapacks::library_dir_at(td.path())).unwrap();
        add(td.path(), entry("gone.zip", "bbb"), PackMcmeta::Unreadable)
            .await
            .unwrap();
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
        add(td.path(), entry("vm.zip", "aaa"), PackMcmeta::Unreadable)
            .await
            .unwrap();

        remove(td.path(), "vm.zip").await.unwrap();
        let raw = std::fs::read_to_string(crate::datapacks::registry_path_at(td.path())).unwrap();
        assert!(!raw.contains("\"vm.zip\""));
        assert!(lib.join("vm.zip").exists());
    }

    #[tokio::test]
    async fn the_version_key_is_written_from_day_one() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(crate::datapacks::library_dir_at(td.path())).unwrap();
        add(td.path(), entry("vm.zip", "aaa"), PackMcmeta::Unreadable)
            .await
            .unwrap();
        let raw = std::fs::read_to_string(crate::datapacks::registry_path_at(td.path())).unwrap();
        // A missing `version` key would default to CURRENT on read, so a file
        // written without it could never be migrated later. Pinned to the
        // literal current version on purpose: this test is what makes a
        // FILE_VERSION bump visible rather than silent.
        assert!(raw.contains("\"version\": 3"), "raw was: {raw}");
    }

    #[tokio::test]
    async fn a_v1_file_upgrades_to_v3_and_keeps_its_row() {
        // Asserting only on the version number would pass straight THROUGH the
        // wipe path: `read_or_empty` is `from_slice(..).unwrap_or(<empty>)`, so
        // a shape it cannot satisfy silently discards every row and reconcile
        // re-adopts the files with `source`/`project_id`/`version_id` all
        // `None` — the loss this module's own doc calls "gone for good", and
        // which now includes the catalog provenance slice 2 adds. The surviving
        // ROW is the assertion that matters.
        let td = tempfile::tempdir().unwrap();
        let lucerna = td.path().join("lucerna");
        std::fs::create_dir_all(&lucerna).unwrap();
        std::fs::write(
            lucerna.join("installed-datapacks.json"),
            r#"{"version":1,"datapacks":[{"filename":"vm.zip","sha1":"abc","size_bytes":10.0,
                "pack_format":48,"name":"Vein Miner","source":"modrinth","project_id":"p1",
                "version_id":"v1","installed_at":"2026-01-01T00:00:00Z"}]}"#,
        )
        .unwrap();
        // The library file must exist or `reconcile` drops the row before the
        // migration is even observable.
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("vm.zip"), b"x").unwrap();

        let listed = list(td.path()).await.unwrap();

        assert_eq!(listed.len(), 1, "the v1 row must survive the upgrade");
        assert_eq!(listed[0].project_id.as_deref(), Some("p1"));
        assert_eq!(listed[0].version_id.as_deref(), Some("v1"));
        assert_eq!(
            listed[0].version_number, None,
            "absent in v1; serde supplies the None, migrate cannot backfill"
        );
        let raw = std::fs::read_to_string(lucerna.join("installed-datapacks.json")).unwrap();
        assert!(raw.contains("\"version\": 3"), "raw was: {raw}");
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
        add(td.path(), entry("vm.zip", "aaa"), PackMcmeta::Unreadable)
            .await
            .unwrap();

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
                add(
                    &root,
                    entry(&format!("p{i}.zip"), "sha"),
                    PackMcmeta::Unreadable,
                )
                .await
            }));
        }
        for h in handles {
            h.await.unwrap().unwrap();
        }

        let got = list(td.path()).await.unwrap();
        assert_eq!(got.len(), 8, "every concurrent add must survive");
    }
}

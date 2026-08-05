//! The `.zip`-aware provenance sidecar for a server world's datapacks.

use std::path::Path;

use crate::error::Result;
use crate::servers_runtime::installed::{self, ServerInstalledRecord};

/// Read the sidecar, reconciled against the world's `datapacks/` dir, and
/// persist it when reconciling changed anything.
///
/// **This deliberately does NOT call `installed::reconcile_on_list`.** That
/// function's `scan_dir` admits only `.jar` / `.jar.disabled` names, so
/// pointed at a `datapacks/` dir it sees zero files and `retain`s every row
/// away against an empty sha set — then saves the wipe. Because the module is
/// fail-open, the loss is silent, and update checking is inert forever after.
/// Never point it at a datapacks dir.
///
/// Two further departures from the jar reconcile, both load-bearing:
///
/// * **Rows are keyed by FILENAME, not sha1.** A jar's bytes never change (a
///   `.disabled` rename preserves them), so sha1 is a stable identity there.
///   A datapack update inverts it: the bytes change while the filename
///   typically stays. Keying on sha1 would drop the row on every update.
/// * **Directories are listed but never adopted.** A folder pack has no bytes
///   to hash and no provenance to record; `listing` synthesizes an ephemeral
///   row for it instead.
pub fn reconcile(world_dir: &Path) -> Vec<ServerInstalledRecord> {
    let dp_dir = world_dir.join("datapacks");
    let mut records = installed::load(world_dir);

    let on_disk: Vec<String> = match std::fs::read_dir(&dp_dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| {
                e.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                    && e.file_name()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(".zip")
            })
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect(),
        // A world that has never had a pack added really does hold none, so
        // retaining against an empty list is the right answer.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        // Anything else — permission denied, a relocated data root caught
        // mid-move, a path that is a file — is NOT proof the dir is empty.
        // Treating it as one would retain every row away and persist that,
        // permanently losing provenance over a problem that was never about
        // the packs. Skip reconciling entirely.
        Err(e) => {
            crate::diag!(
                "server datapacks: reconcile skipped, could not read {}: {e}",
                dp_dir.display()
            );
            return records;
        }
    };

    let folded: Vec<String> = on_disk.iter().map(|n| n.to_lowercase()).collect();
    let before = records.len();
    records.retain(|r| folded.contains(&r.filename.to_lowercase()));
    let mut changed = records.len() != before;

    for name in on_disk {
        if records
            .iter()
            .any(|r| r.filename.to_lowercase() == name.to_lowercase())
        {
            continue;
        }
        // A merely-unreadable file (AV hold, deleted between the listing and
        // this read) must not be adopted as a fabricated zero-byte row — that
        // presents a guess as fact. Skip it; the next listing tries again.
        let Ok(sha1) = installed::sha1_of(&dp_dir.join(&name)) else {
            crate::diag!("server datapacks: skipping adoption of {name}, unreadable");
            continue;
        };
        records.push(ServerInstalledRecord {
            filename: name,
            sha1,
            source: None,
            project_id: None,
            version_id: None,
            name: None,
            version_number: None,
            enrich_attempted: false,
        });
        changed = true;
    }

    if changed {
        // Best-effort: a read-only data root must not turn a good listing into
        // an error page. The in-memory rows are already correct.
        if let Err(e) = installed::save(world_dir, &records) {
            crate::diag!("server datapacks: sidecar save failed: {e}");
        }
    }
    records
}

/// Insert or replace the row for `record.filename`, matching case-insensitively
/// with FULL Unicode folding — NTFS folds Cyrillic too, so `Пак.zip` and
/// `пак.zip` address one file and must never produce two rows.
///
/// Deliberately not `installed::upsert`, which dedups by sha1: the datapack
/// case is one filename whose bytes just changed.
pub fn upsert_by_filename(world_dir: &Path, record: ServerInstalledRecord) -> Result<()> {
    let key = record.filename.to_lowercase();
    let mut records = installed::load(world_dir);
    records.retain(|r| r.filename.to_lowercase() != key);
    records.push(record);
    installed::save(world_dir, &records)
}

/// Drop the row for `filename`. Idempotent; writes only when something went.
pub fn forget(world_dir: &Path, filename: &str) -> Result<()> {
    let key = filename.to_lowercase();
    let mut records = installed::load(world_dir);
    let before = records.len();
    records.retain(|r| r.filename.to_lowercase() != key);
    if records.len() != before {
        installed::save(world_dir, &records)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::ModSource;
    use std::io::Write;

    fn datapack_zip(body: &[u8]) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(br#"{"pack":{"pack_format":48,"description":"Pack"}}"#)
            .unwrap();
        zw.start_file("data/ns/function/tick.mcfunction", opts)
            .unwrap();
        zw.write_all(body).unwrap();
        zw.finish().unwrap().into_inner()
    }

    /// A world dir with a `datapacks/` child holding the named zips.
    fn world_with(packs: &[(&str, &[u8])]) -> tempfile::TempDir {
        let td = tempfile::tempdir().unwrap();
        let dp = td.path().join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        for (name, body) in packs {
            std::fs::write(dp.join(name), datapack_zip(body)).unwrap();
        }
        td
    }

    fn row(filename: &str, sha1: &str) -> ServerInstalledRecord {
        ServerInstalledRecord {
            filename: filename.into(),
            sha1: sha1.into(),
            source: Some(ModSource::Modrinth),
            project_id: Some("terralith".into()),
            version_id: Some("v1".into()),
            name: Some("Terralith".into()),
            version_number: Some("2.5.0".into()),
            enrich_attempted: false,
        }
    }

    #[test]
    fn a_hand_dropped_zip_is_adopted_as_a_provenance_less_row() {
        let td = world_with(&[("hand.zip", b"x")]);
        let rows = reconcile(td.path());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "hand.zip");
        assert!(
            rows[0].source.is_none(),
            "adoption must not invent provenance"
        );
        assert_eq!(rows[0].sha1.len(), 40, "an adopted row carries a real sha1");
    }

    #[test]
    fn a_catalog_row_keeps_its_provenance_when_the_bytes_change() {
        // The whole reason this cannot be `installed::reconcile_on_list`: a
        // datapack update REPLACES bytes under an unchanged filename, so a
        // sha1-keyed retain would drop the row and make update checking inert.
        let td = world_with(&[("terralith.zip", b"v1")]);
        let sha_v1 = crate::servers_runtime::installed::sha1_of(
            &td.path().join("datapacks").join("terralith.zip"),
        )
        .unwrap();
        crate::servers_runtime::installed::save(td.path(), &[row("terralith.zip", &sha_v1)])
            .unwrap();

        std::fs::write(
            td.path().join("datapacks").join("terralith.zip"),
            datapack_zip(b"v2"),
        )
        .unwrap();

        let rows = reconcile(td.path());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project_id.as_deref(), Some("terralith"));
    }

    #[test]
    fn a_row_whose_file_is_gone_is_pruned() {
        let td = world_with(&[]);
        crate::servers_runtime::installed::save(td.path(), &[row("gone.zip", "deadbeef")]).unwrap();
        assert!(reconcile(td.path()).is_empty());
        assert!(crate::servers_runtime::installed::load(td.path()).is_empty());
    }

    #[test]
    fn a_folder_pack_is_never_adopted_into_the_sidecar() {
        let td = world_with(&[]);
        // Named WITH a `.zip` suffix on purpose: it is the `.ends_with(".zip")`
        // filter that would (wrongly) admit this entry if `is_file()` were ever
        // dropped from the on-disk filter — a fixture without the suffix would
        // pass this test for the wrong reason, hiding that regression.
        std::fs::create_dir_all(td.path().join("datapacks").join("FolderPack.zip")).unwrap();
        assert!(
            reconcile(td.path()).is_empty(),
            "a folder has no bytes to hash and no provenance to record — it is \
             listed from disk, never persisted"
        );
    }

    #[test]
    fn reconcile_folds_case_when_matching_a_row_to_its_file() {
        // NTFS resolves `Terralith.zip` and `terralith.zip` to one file, so a
        // sidecar row whose spelling drifted from the directory entry must be
        // retained with its provenance — not pruned and re-adopted bare.
        let td = world_with(&[("terralith.zip", b"v1")]);
        crate::servers_runtime::installed::save(td.path(), &[row("Terralith.zip", "aa")]).unwrap();
        let rows = reconcile(td.path());
        assert_eq!(rows.len(), 1, "one file must never yield two rows");
        assert_eq!(
            rows[0].project_id.as_deref(),
            Some("terralith"),
            "the drifted-spelling row keeps its provenance"
        );
    }

    #[test]
    fn an_unreadable_datapacks_dir_never_wipes_the_sidecar() {
        // Contrast with an ABSENT dir, where retaining against an empty disk
        // list is correct. A read error is not proof the packs are gone, and
        // persisting a wipe would lose every pack's provenance permanently.
        let td = tempfile::tempdir().unwrap();
        // No `datapacks/` dir at all, but a FILE by that name — read_dir on it
        // fails with NotADirectory, not NotFound.
        std::fs::write(td.path().join("datapacks"), b"not a dir").unwrap();
        crate::servers_runtime::installed::save(td.path(), &[row("keep.zip", "aa")]).unwrap();
        let rows = reconcile(td.path());
        assert_eq!(rows.len(), 1, "rows survive an unreadable dir");
        assert_eq!(crate::servers_runtime::installed::load(td.path()).len(), 1);
    }

    #[test]
    fn upsert_by_filename_replaces_the_row_for_that_name_case_insensitively() {
        let td = world_with(&[]);
        crate::servers_runtime::installed::save(td.path(), &[row("Пак.zip", "aa")]).unwrap();
        let mut next = row("пак.zip", "bb");
        next.version_id = Some("v2".into());
        upsert_by_filename(td.path(), next).unwrap();
        let rows = crate::servers_runtime::installed::load(td.path());
        assert_eq!(rows.len(), 1, "NTFS folds Cyrillic — one file, one row");
        assert_eq!(rows[0].version_id.as_deref(), Some("v2"));
    }

    #[test]
    fn forget_drops_the_row_for_a_name_and_is_idempotent() {
        let td = world_with(&[]);
        crate::servers_runtime::installed::save(td.path(), &[row("a.zip", "aa")]).unwrap();
        forget(td.path(), "A.ZIP").unwrap();
        assert!(crate::servers_runtime::installed::load(td.path()).is_empty());
        forget(td.path(), "a.zip").unwrap();
    }
}

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
/// * **Only what the game loads is adopted (D1):** a file whose name ends in
///   exactly `.zip` and whose zip has `pack.mcmeta` at its root. A row is
///   RETAINED against every regular file, so a legacy row for a file the game
///   ignores keeps its provenance.
///
/// Holds the sidecar lock for the whole pass, the dir listing included — same
/// reason as `installed::reconcile_on_list`: a catalog install whose zip lands
/// after an unlocked listing and whose row lands before the load would be
/// pruned as "gone", and saved.
pub fn reconcile(world_dir: &Path) -> Vec<ServerInstalledRecord> {
    let sidecar = installed::lock(world_dir);
    let dp_dir = world_dir.join("datapacks");
    // An UNREADABLE sidecar (`load` discriminates that from an absent one,
    // which reads as empty) is ignorance, not absence: reconciling against it
    // would adopt every on-disk zip as a provenance-less row and persist that
    // wipe. Skip the pass entirely — the listing degrades to ephemeral rows
    // for one round and the next one retries.
    let mut records = match sidecar.load() {
        Ok(records) => records,
        Err(e) => {
            crate::diag!(
                "server datapacks: reconcile skipped, could not read sidecar in {}: {e}",
                world_dir.display()
            );
            return Vec::new();
        }
    };

    let files: Vec<String> = match std::fs::read_dir(&dp_dir).and_then(|rd| {
        regular_file_names(rd.map(|e| {
            e.map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                (name, e.file_type().map(|ft| ft.is_file()))
            })
        }))
    }) {
        Ok(files) => files,
        // A world that has never had a pack added really does hold none, so
        // retaining against an empty list is the right answer.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        // Anything else — permission denied, a relocated data root caught
        // mid-move, a path that is a file, one entry (or its type) that could
        // not be read — is NOT proof the files are gone. Treating it as one
        // would retain rows away and persist that, permanently losing
        // provenance over a problem that was never about the packs. Skip
        // reconciling entirely.
        Err(e) => {
            crate::diag!(
                "server datapacks: reconcile skipped, could not read {}: {e}",
                dp_dir.display()
            );
            return records;
        }
    };

    // R2 (N.3): the on-disk file each row names. A row whose name could not
    // be resolved is kept: a pruned row is provenance lost for good. A row
    // that names its file in another case takes the on-disk spelling, which is
    // the pack's id — the listing, the pane's row key and a client made from
    // this server then all see the name the game loads. Two rows can name one
    // file that way (`VM.zip` and `vm.zip` on NTFS): the one naming it exactly
    // stays, else the first, and a duplicate is dropped rather than saved.
    use crate::datapacks::detect::{resolve, Resolved};
    let resolutions: Vec<Resolved> = records
        .iter()
        .map(|r| resolve(&dp_dir, &r.filename, &files))
        .collect();
    let named_exactly: Vec<String> = resolutions
        .iter()
        .filter_map(|res| match res {
            Resolved::Exact(n) => Some(n.clone()),
            _ => None,
        })
        .collect();
    let mut claimed: Vec<String> = Vec::new();
    let mut changed = false;
    let mut kept = Vec::with_capacity(records.len());
    for (mut r, res) in records.into_iter().zip(resolutions) {
        match res {
            Resolved::Exact(n) => {
                claimed.push(n);
                kept.push(r);
            }
            Resolved::Folded(n) => {
                changed = true;
                if named_exactly.contains(&n) || claimed.contains(&n) {
                    crate::diag!(
                        "server datapacks: dropped the row for {}: another row already names {n}",
                        r.filename
                    );
                    continue;
                }
                r.filename = n.clone();
                claimed.push(n);
                kept.push(r);
            }
            Resolved::Absent => changed = true,
            Resolved::Unknown(e) => {
                crate::diag!(
                    "server datapacks: kept the row for {}, could not tell whether its file is there: {e}",
                    r.filename
                );
                kept.push(r);
            }
        }
    }
    records = kept;

    for name in files {
        // D1: adopt only what the game loads: exact `.zip` with a root pack.mcmeta.
        if claimed.contains(&name) || !crate::datapacks::detect::has_zip_suffix(&name) {
            continue;
        }
        match crate::datapacks::detect::zip_has_root_pack_mcmeta(&dp_dir.join(&name)) {
            Ok(true) => {}
            // World content, not a pack; the listing shows it Ignored.
            Ok(false) => continue,
            Err(e) => {
                crate::diag!(
                    "server datapacks: skipping adoption of {name}, could not check its root: {e}"
                );
                continue;
            }
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
        if let Err(e) = sidecar.save(&records) {
            crate::diag!("server datapacks: sidecar save failed: {e}");
        }
    }
    records
}

/// The regular files among `dp_dir`'s entries, from `(name, is_file)`
/// pairs. Any entry, or its type, that could not be read is an `Err`: the
/// caller must not prune rows against a partial list (Fallback discipline Q2).
fn regular_file_names<I>(entries: I) -> std::io::Result<Vec<String>>
where
    I: IntoIterator<Item = std::io::Result<(String, std::io::Result<bool>)>>,
{
    let mut out = Vec::new();
    for entry in entries {
        let (name, is_file) = entry?;
        if is_file? {
            out.push(name);
        }
    }
    Ok(out)
}

/// Insert or replace the row for `record.filename`. A row is replaced when it
/// has the same name, or when R2 (spec §2 N.3) resolves it to the same
/// on-disk entry (NTFS/APFS: `Пак.zip` and `пак.zip` address one file and
/// must never produce two rows), or when it differs only in case and names a
/// file that is gone. On a case-sensitive file system a present case variant
/// is a different pack, and its row stays.
///
/// Deliberately not `installed::upsert`, which dedups by sha1: the datapack
/// case is one filename whose bytes just changed.
pub fn upsert_by_filename(world_dir: &Path, record: ServerInstalledRecord) -> Result<()> {
    let sidecar = installed::lock(world_dir);
    let dp_dir = world_dir.join("datapacks");
    let names = entry_names_or_log(&dp_dir);
    let mut records = sidecar.load()?;
    records.retain(|r| !same_or_gone(&dp_dir, names.as_deref(), &record.filename, &r.filename));
    records.push(record);
    sidecar.save(&records)
}

/// Drop the row for `filename`, matched as in [`upsert_by_filename`].
/// Idempotent; writes only when something went.
pub fn forget(world_dir: &Path, filename: &str) -> Result<()> {
    let sidecar = installed::lock(world_dir);
    let dp_dir = world_dir.join("datapacks");
    let names = entry_names_or_log(&dp_dir);
    let mut records = sidecar.load()?;
    let before = records.len();
    records.retain(|r| !same_or_gone(&dp_dir, names.as_deref(), filename, &r.filename));
    if records.len() != before {
        sidecar.save(&records)?;
    }
    Ok(())
}

/// The world's `datapacks/` names for [`same_or_gone`]. `None` when they
/// could not be read: then only the exact name matches, the restrictive answer.
fn entry_names_or_log(dp_dir: &Path) -> Option<Vec<String>> {
    match crate::datapacks::detect::entry_names(dp_dir) {
        Ok(n) => Some(n),
        Err(e) => {
            crate::diag!(
                "server datapacks: could not list {}: {e}; matching the exact name only",
                dp_dir.display()
            );
            None
        }
    }
}

/// N.3 for sidecar rows: does `row_name` denote the same on-disk entry as
/// `name`, or no entry at all? Only a row equal to `name` case-insensitively
/// is ever a candidate. On a case-sensitive file system a present case
/// variant is a different pack, and its row stays. Could not tell ⇒ the row stays.
fn same_or_gone(dp_dir: &Path, names: Option<&[String]>, name: &str, row_name: &str) -> bool {
    use crate::datapacks::detect::{resolve, Resolved};
    if row_name == name {
        return true;
    }
    let Some(names) = names else {
        return false;
    };
    if row_name.to_lowercase() != name.to_lowercase() {
        return false;
    }
    let entry = |n: &str| match resolve(dp_dir, n, names) {
        Resolved::Exact(e) | Resolved::Folded(e) => Ok(Some(e)),
        Resolved::Absent => Ok(None),
        Resolved::Unknown(e) => Err(e),
    };
    match (entry(row_name), entry(name)) {
        // The row's file is gone.
        (Ok(None), _) => true,
        // One entry, two spellings (NTFS/APFS).
        (Ok(Some(r)), Ok(Some(t))) => r == t,
        _ => false,
    }
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
        crate::servers_runtime::installed::lock(td.path())
            .save(&[row("terralith.zip", &sha_v1)])
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
        crate::servers_runtime::installed::lock(td.path())
            .save(&[row("gone.zip", "deadbeef")])
            .unwrap();
        assert!(reconcile(td.path()).is_empty());
        assert!(crate::servers_runtime::installed::lock(td.path())
            .load()
            .unwrap()
            .is_empty());
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

    /// R2 (N.3). NTFS/APFS resolve `Terralith.zip` to the file
    /// `terralith.zip`, so a sidecar row whose spelling drifted from the
    /// directory entry keeps its provenance. On a case-sensitive file system
    /// the row names an absent file: it is pruned, and the file is adopted bare.
    #[test]
    fn reconcile_folds_case_when_matching_a_row_to_its_file() {
        let td = world_with(&[("terralith.zip", b"v1")]);
        crate::servers_runtime::installed::lock(td.path())
            .save(&[row("Terralith.zip", "aa")])
            .unwrap();
        let rows = reconcile(td.path());
        assert_eq!(rows.len(), 1, "one file must never yield two rows");
        if crate::datapacks::detect::test_support::fs_folds_case(td.path()) {
            assert_eq!(
                rows[0].project_id.as_deref(),
                Some("terralith"),
                "the drifted-spelling row keeps its provenance"
            );
            // The row takes the on-disk spelling, which is the pack's id, so
            // the listing, the pane's row key and a client made from this
            // server all see the name the game loads — and it is saved.
            assert_eq!(rows[0].filename, "terralith.zip");
            let saved = crate::servers_runtime::installed::lock(td.path())
                .load()
                .unwrap();
            assert_eq!(saved[0].filename, "terralith.zip");
        } else {
            assert_eq!(rows[0].project_id, None);
            assert_eq!(rows[0].filename, "terralith.zip");
        }
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
        crate::servers_runtime::installed::lock(td.path())
            .save(&[row("keep.zip", "aa")])
            .unwrap();
        let rows = reconcile(td.path());
        assert_eq!(rows.len(), 1, "rows survive an unreadable dir");
        assert_eq!(
            crate::servers_runtime::installed::lock(td.path())
                .load()
                .unwrap()
                .len(),
            1
        );
    }

    /// R2 (N.3) with the file present: NTFS/APFS fold Cyrillic, so `Пак.zip`
    /// and `пак.zip` are one file and one row. On a case-sensitive file
    /// system the new spelling is a different pack, and both rows stay.
    #[test]
    fn upsert_by_filename_replaces_the_row_for_that_name_case_insensitively() {
        let td = world_with(&[("Пак.zip", b"a")]);
        crate::servers_runtime::installed::lock(td.path())
            .save(&[row("Пак.zip", "aa")])
            .unwrap();
        let mut next = row("пак.zip", "bb");
        next.version_id = Some("v2".into());
        upsert_by_filename(td.path(), next).unwrap();
        let rows = crate::servers_runtime::installed::lock(td.path())
            .load()
            .unwrap();
        if crate::datapacks::detect::test_support::fs_folds_case(td.path()) {
            assert_eq!(rows.len(), 1, "one file, one row");
            assert_eq!(rows[0].version_id.as_deref(), Some("v2"));
        } else {
            assert_eq!(rows.len(), 2, "two spellings are two packs here");
        }
    }

    /// The datapack browser installs several catalog packs at once (a per-card
    /// busy set), each ending in `upsert_by_filename` on a tokio worker. Two
    /// writers that read one snapshot lose a row — and its provenance — to the
    /// later rename.
    #[test]
    fn concurrent_filename_upserts_keep_every_datapack_row() {
        const WRITERS: usize = 16;
        const PER_WRITER: usize = 25;
        let td = world_with(&[]);
        let start = std::sync::Barrier::new(WRITERS);
        std::thread::scope(|s| {
            for w in 0..WRITERS {
                let (world, start) = (td.path(), &start);
                s.spawn(move || {
                    start.wait();
                    for i in 0..PER_WRITER {
                        upsert_by_filename(world, row(&format!("pack-{w}-{i}.zip"), "aa")).unwrap();
                    }
                });
            }
        });
        assert_eq!(
            crate::servers_runtime::installed::lock(td.path())
                .load()
                .unwrap()
                .len(),
            WRITERS * PER_WRITER,
            "a concurrent upsert erased another pack's row"
        );
    }

    #[test]
    fn forget_drops_the_row_for_a_name_and_is_idempotent() {
        let td = world_with(&[]);
        crate::servers_runtime::installed::lock(td.path())
            .save(&[row("a.zip", "aa")])
            .unwrap();
        forget(td.path(), "A.ZIP").unwrap();
        assert!(crate::servers_runtime::installed::lock(td.path())
            .load()
            .unwrap()
            .is_empty());
        forget(td.path(), "a.zip").unwrap();
    }

    #[test]
    fn a_rootless_zip_is_not_adopted() {
        let td = world_with(&[]);
        std::fs::write(
            td.path().join("datapacks/rootless.zip"),
            crate::datapacks::detect::test_support::zip_of(&[("readme.txt", b"x")]),
        )
        .unwrap();
        assert!(reconcile(td.path()).is_empty());
    }

    #[test]
    fn an_upper_case_zip_is_not_adopted() {
        let td = world_with(&[("Upper.ZIP", b"x")]);
        assert!(
            reconcile(td.path()).is_empty(),
            "the game ignores it; the listing shows it Ignored"
        );
    }

    /// A legacy row for a file the game ignores keeps its provenance: retain
    /// runs against every regular file, and only adoption is exact.
    #[test]
    fn a_legacy_upper_case_row_keeps_its_provenance() {
        let td = world_with(&[("Old.ZIP", b"x")]);
        crate::servers_runtime::installed::lock(td.path())
            .save(&[row("Old.ZIP", "aa")])
            .unwrap();
        let rows = reconcile(td.path());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project_id.as_deref(), Some("terralith"));
    }

    #[test]
    fn upsert_keeps_a_present_case_variant_row() {
        let td = world_with(&[("Terra.zip", b"a")]);
        if crate::datapacks::detect::test_support::fs_folds_case(td.path()) {
            return;
        }
        std::fs::write(td.path().join("datapacks/terra.zip"), datapack_zip(b"b")).unwrap();
        crate::servers_runtime::installed::lock(td.path())
            .save(&[row("Terra.zip", "aa")])
            .unwrap();
        upsert_by_filename(td.path(), row("terra.zip", "bb")).unwrap();
        assert_eq!(
            crate::servers_runtime::installed::lock(td.path())
                .load()
                .unwrap()
                .len(),
            2
        );
    }

    /// Fallback Q2: an entry of `datapacks/` that cannot be read (or whose
    /// type cannot) is not "no such file". Pruning against a partial list
    /// would drop that pack's row and its provenance for good, so the pass is
    /// skipped instead.
    #[test]
    fn an_entry_that_cannot_be_read_fails_the_file_list() {
        let entries: Vec<std::io::Result<(String, std::io::Result<bool>)>> = vec![
            Ok(("a.zip".into(), Ok(true))),
            Ok((
                "b.zip".into(),
                Err(std::io::Error::other("type unreadable")),
            )),
        ];
        assert!(regular_file_names(entries).is_err());
        let entries: Vec<std::io::Result<(String, std::io::Result<bool>)>> = vec![
            Ok(("a.zip".into(), Ok(true))),
            Err(std::io::Error::other("entry unreadable")),
        ];
        assert!(regular_file_names(entries).is_err());
        let entries: Vec<std::io::Result<(String, std::io::Result<bool>)>> = vec![
            Ok(("a.zip".into(), Ok(true))),
            Ok(("dir".into(), Ok(false))),
        ];
        assert_eq!(
            regular_file_names(entries).unwrap(),
            vec!["a.zip".to_string()]
        );
    }

    /// Two rows that name one file (NTFS/APFS: `VM.zip` folds onto the file
    /// `vm.zip`, which `vm.zip` names exactly) must not both take its spelling
    /// and be saved as duplicates. The exact row wins; with no exact row, the
    /// first one does. On a case-sensitive file system `VM.zip` names no file
    /// and is pruned. Either way: one row, spelled as on disk.
    #[test]
    fn two_rows_naming_one_file_leave_one_row() {
        let td = world_with(&[("vm.zip", b"a")]);
        let mut exact = row("vm.zip", "bb");
        exact.project_id = Some("exact".into());
        crate::servers_runtime::installed::lock(td.path())
            .save(&[row("VM.zip", "aa"), exact])
            .unwrap();
        let rows = reconcile(td.path());
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].filename, "vm.zip");
        assert_eq!(rows[0].project_id.as_deref(), Some("exact"));

        let td = world_with(&[("vm.zip", b"a")]);
        crate::servers_runtime::installed::lock(td.path())
            .save(&[row("VM.zip", "aa"), row("Vm.zip", "cc")])
            .unwrap();
        let rows = reconcile(td.path());
        assert!(rows.len() <= 1, "{rows:?}");
        let saved = crate::servers_runtime::installed::lock(td.path())
            .load()
            .unwrap();
        assert_eq!(saved.len(), rows.len(), "{saved:?}");
    }
}

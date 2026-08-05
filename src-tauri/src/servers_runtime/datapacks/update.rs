//! The single-world datapack update.

use std::path::Path;

use crate::datapacks::{level_dat, level_dat_entry, world_link, DatapackProvenance};
use crate::error::{Error, Result};
use crate::servers_runtime::installed;

use super::{level_dat_lock, mutate, sidecar, ServerDatapackUpdateOutcome};

/// Apply one resolved update. `bytes` is the target's verified content; the
/// caller owns download, size caps and verification.
///
/// The step order is the client's post-audit ordering, which is what makes a
/// retry converge, plus the audit's two identity checks and its sidecar
/// ordering. Do not reorder without re-reading the design's §4.4.
pub async fn update_one(
    world_dir: &Path,
    old_filename: &str,
    new_filename: &str,
    bytes: &[u8],
    provenance: &DatapackProvenance,
) -> Result<ServerDatapackUpdateOutcome> {
    if !crate::pathsafe::is_safe_filename(old_filename) {
        return Err(Error::ModsUnsafeFilename {
            filename: old_filename.to_string(),
        });
    }
    let dp_dir = world_dir.join("datapacks");

    // FULL Unicode folding, not `eq_ignore_ascii_case`: NTFS's $UpCase covers
    // Cyrillic and friends, so `Пак.zip` → `пак.zip` addresses the same file
    // exactly as `VM.zip` → `vm.zip` does. Treating that as a rename would run
    // the migrate-and-delete path against the file just written.
    if new_filename.to_lowercase() == old_filename.to_lowercase() {
        // Identity FIRST: the on-disk file must be the one the sidecar
        // describes. A mismatch (hand-replaced pack, or a fail-open-empty
        // sidecar) is reported, and nothing is touched.
        let row = sidecar::reconcile(world_dir)
            .into_iter()
            .find(|r| r.filename.to_lowercase() == old_filename.to_lowercase());
        let disk_sha = installed::sha1_of(&dp_dir.join(old_filename)).unwrap_or_default();
        let matches = row
            .as_ref()
            .is_some_and(|r| r.sha1.eq_ignore_ascii_case(&disk_sha) && !r.sha1.is_empty());
        if !matches {
            return Err(Error::ModsFilenameConflict {
                filename: old_filename.to_string(),
                existing_sha: disk_sha,
                incoming_sha: row.map(|r| r.sha1).unwrap_or_default(),
            });
        }
        // Install under the OLD (registered) spelling so the sidecar and the
        // directory entry stay consistent on every platform.
        let record =
            mutate::install_bytes(world_dir, old_filename, bytes, Some(provenance)).await?;
        return Ok(ServerDatapackUpdateOutcome {
            record,
            was_enabled: None,
            old_removed: true,
            completed: true,
        });
    }

    // (a) The NEW-name slot. An entry that is not byte-identical to the target
    // — a directory always is not — is a conflict; the client's §8.5 defect 6
    // re-enters through this slot otherwise.
    if let Ok(meta) = std::fs::metadata(dp_dir.join(new_filename)) {
        let incoming = crate::datapacks::library::sha1_hex(bytes);
        let existing = if meta.is_dir() {
            String::new()
        } else {
            installed::sha1_of(&dp_dir.join(new_filename)).unwrap_or_default()
        };
        if meta.is_dir() || !existing.eq_ignore_ascii_case(&incoming) {
            return Err(Error::ModsFilenameConflict {
                filename: new_filename.to_string(),
                existing_sha: existing,
                incoming_sha: incoming,
            });
        }
    }

    // (b) Place the new file and upsert its row IMMEDIATELY. A crash after
    // this leaves both rows, which the `.zip`-aware reconcile prunes when a
    // file vanishes; deferring the row to the end would let a crash strip
    // provenance and make update checking silently inert.
    //
    // `install_bytes` takes no level.dat lock, so calling it here and taking
    // the lock below is safe — the module's non-reentrancy rule is about
    // composing two lock-taking entry points.
    let record = mutate::install_bytes(world_dir, new_filename, bytes, Some(provenance)).await?;

    let was_enabled = {
        let _guard = level_dat_lock().lock().await;
        // (c) Read level.dat once. An already-listed NEW entry's state takes
        // precedence: a retried migration must not have its state re-derived
        // from a stale old entry and flip a disabled pack on.
        let (mut root, framing) = world_link::read_level_dat_or_empty(world_dir)?;
        let (enabled_raw, disabled_raw) = level_dat::lists(&root);
        let strip = |v: &[String]| -> Vec<String> {
            v.iter()
                .filter_map(|n| n.strip_prefix("file/").map(str::to_string))
                .collect()
        };
        let (enabled, disabled) = (strip(&enabled_raw), strip(&disabled_raw));
        let new_listed = world_link::contains_ci(&enabled, new_filename)
            || world_link::contains_ci(&disabled, new_filename);
        // The three-case rule: in neither list means ENABLED (Minecraft
        // auto-enables a present, unlisted pack). Reading it as two cases
        // silently disables a pack that was on.
        let was_enabled = if new_listed {
            !world_link::contains_ci(&disabled, new_filename)
        } else {
            !world_link::contains_ci(&disabled, old_filename)
        };
        let mut changed = level_dat::forget_ci(&mut root, &level_dat_entry(old_filename))?;
        changed |= level_dat::set_enabled(&mut root, &level_dat_entry(new_filename), was_enabled)?;
        if changed {
            level_dat::write_at(world_dir, &root, framing).await?;
        }
        was_enabled
    };

    // (d) Identity, THEN delete the old file, LAST. A failure at any earlier
    // step leaves the old file present, so a re-run still finds the work.
    let old_row = sidecar::reconcile(world_dir)
        .into_iter()
        .find(|r| r.filename.to_lowercase() == old_filename.to_lowercase());
    let old_path = dp_dir.join(old_filename);
    let old_disk_sha = installed::sha1_of(&old_path).unwrap_or_default();
    let old_is_ours = old_row
        .as_ref()
        .is_some_and(|r| !r.sha1.is_empty() && r.sha1.eq_ignore_ascii_case(&old_disk_sha));
    if !old_is_ours && old_path.exists() {
        // A hand-replaced or sidecar-unknown file. Leave it; report honestly.
        return Ok(ServerDatapackUpdateOutcome {
            record,
            was_enabled: Some(was_enabled),
            old_removed: false,
            completed: false,
        });
    }
    match std::fs::remove_file(&old_path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::io(old_path.display().to_string(), e)),
    }

    // (e) Drop the OLD sidecar row last.
    sidecar::forget(world_dir, old_filename)?;

    Ok(ServerDatapackUpdateOutcome {
        record,
        was_enabled: Some(was_enabled),
        old_removed: true,
        completed: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::{level_dat, level_dat_entry, WorldPackState};
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

    fn prov(version: &str) -> crate::datapacks::DatapackProvenance {
        crate::datapacks::DatapackProvenance {
            source: crate::mods::platform::ModSource::Modrinth,
            project_id: "veinminer".into(),
            version_id: version.into(),
            version_number: Some(format!("{version}.0")),
        }
    }

    async fn booted_world_with(old: &str, body: &[u8]) -> tempfile::TempDir {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(td.path().join("datapacks")).unwrap();
        let mut data = std::collections::HashMap::new();
        data.insert(
            "LevelName".to_string(),
            fastnbt::Value::String("srv".into()),
        );
        let mut root = std::collections::HashMap::new();
        root.insert("Data".to_string(), fastnbt::Value::Compound(data));
        level_dat::write_at(
            td.path(),
            &fastnbt::Value::Compound(root),
            level_dat::Framing::Gzip,
        )
        .await
        .unwrap();
        crate::servers_runtime::datapacks::mutate::install_bytes(
            td.path(),
            old,
            &datapack_zip(body),
            Some(&prov("v1")),
        )
        .await
        .unwrap();
        td
    }

    fn state_of(world_dir: &std::path::Path, name: &str) -> Option<WorldPackState> {
        crate::servers_runtime::datapacks::listing::entries(world_dir)
            .into_iter()
            .find(|e| e.record.filename == name)
            .and_then(|e| e.state)
    }

    #[tokio::test]
    async fn an_unchanged_filename_replaces_bytes_and_never_touches_level_dat() {
        let td = booted_world_with("vm.zip", b"v1").await;
        crate::servers_runtime::datapacks::mutate::set_enabled(td.path(), "vm.zip", false)
            .await
            .unwrap();
        let before = std::fs::read(td.path().join("level.dat")).unwrap();

        let out = update_one(
            td.path(),
            "vm.zip",
            "vm.zip",
            &datapack_zip(b"v2"),
            &prov("v2"),
        )
        .await
        .unwrap();

        assert!(out.completed);
        assert_eq!(
            out.was_enabled, None,
            "this path does not read the state, so it must not claim one"
        );
        assert_eq!(
            std::fs::read(td.path().join("level.dat")).unwrap(),
            before,
            "level.dat must be byte-identical"
        );
        assert_eq!(
            std::fs::read(td.path().join("datapacks").join("vm.zip")).unwrap(),
            datapack_zip(b"v2")
        );
        let rows = crate::servers_runtime::installed::load(td.path());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].version_id.as_deref(), Some("v2"));
    }

    #[tokio::test]
    async fn a_renamed_update_keeps_a_disabled_pack_disabled_with_exactly_one_entry() {
        let td = booted_world_with("vm-1.0.zip", b"v1").await;
        crate::servers_runtime::datapacks::mutate::set_enabled(td.path(), "vm-1.0.zip", false)
            .await
            .unwrap();

        let out = update_one(
            td.path(),
            "vm-1.0.zip",
            "vm-2.0.zip",
            &datapack_zip(b"v2"),
            &prov("v2"),
        )
        .await
        .unwrap();

        assert!(out.completed && out.old_removed);
        assert_eq!(out.was_enabled, Some(false));
        assert_eq!(
            state_of(td.path(), "vm-2.0.zip"),
            Some(WorldPackState::Disabled)
        );
        assert!(!td.path().join("datapacks").join("vm-1.0.zip").exists());
        let (root, _) = level_dat::read_at(td.path()).unwrap();
        let (en, dis) = level_dat::lists(&root);
        assert_eq!(en.len() + dis.len(), 1, "exactly one entry, not both names");
        assert!(dis.contains(&level_dat_entry("vm-2.0.zip")));
        let rows = crate::servers_runtime::installed::load(td.path());
        assert_eq!(rows.len(), 1, "the old sidecar row is dropped last");
    }

    #[tokio::test]
    async fn a_renamed_update_of_an_enabled_pack_keeps_it_enabled() {
        // The three-case rule: a pack in NEITHER list is ENABLED, because
        // Minecraft auto-enables a present, unlisted pack. Reading it as two
        // cases would silently disable a pack that was on.
        let td = booted_world_with("vm-1.0.zip", b"v1").await;
        let out = update_one(
            td.path(),
            "vm-1.0.zip",
            "vm-2.0.zip",
            &datapack_zip(b"v2"),
            &prov("v2"),
        )
        .await
        .unwrap();
        assert_eq!(out.was_enabled, Some(true));
        assert_eq!(
            state_of(td.path(), "vm-2.0.zip"),
            Some(WorldPackState::Enabled)
        );
    }

    #[tokio::test]
    async fn a_hand_replaced_old_file_is_left_alone_and_reported() {
        // Audit #3: the identity check before deleting the old file. The
        // sidecar row is the witness — the reconcile is filename-keyed and
        // never rehashes a retained row, so an install-time sha1 survives and
        // really does detect a replacement.
        let td = booted_world_with("vm-1.0.zip", b"v1").await;
        std::fs::write(
            td.path().join("datapacks").join("vm-1.0.zip"),
            datapack_zip(b"the admin edited this"),
        )
        .unwrap();

        let out = update_one(
            td.path(),
            "vm-1.0.zip",
            "vm-2.0.zip",
            &datapack_zip(b"v2"),
            &prov("v2"),
        )
        .await
        .unwrap();

        assert!(!out.old_removed && !out.completed);
        assert!(
            td.path().join("datapacks").join("vm-1.0.zip").exists(),
            "a pack the admin replaced by hand must never be deleted"
        );
    }

    #[tokio::test]
    async fn a_foreign_entry_in_the_new_name_slot_is_a_conflict_with_nothing_touched() {
        // Audit #3, the other half: §8.5 defect 6 re-enters through this slot.
        let td = booted_world_with("vm-1.0.zip", b"v1").await;
        std::fs::create_dir_all(td.path().join("datapacks").join("vm-2.0.zip")).unwrap();

        let err = update_one(
            td.path(),
            "vm-1.0.zip",
            "vm-2.0.zip",
            &datapack_zip(b"v2"),
            &prov("v2"),
        )
        .await
        .unwrap_err();

        assert!(
            matches!(err, Error::ModsFilenameConflict { .. }),
            "got {err:?}"
        );
        assert!(td.path().join("datapacks").join("vm-1.0.zip").exists());
    }

    #[tokio::test]
    async fn the_new_rows_provenance_survives_a_crash_before_the_old_row_is_dropped() {
        // Audit #4: the new sidecar row is upserted right after the file is
        // placed. Simulate the crash by re-listing at that point — both rows
        // exist, and the .zip-aware reconcile prunes the one whose file is
        // gone WITHOUT stripping the new row's provenance.
        let td = booted_world_with("vm-1.0.zip", b"v1").await;
        crate::servers_runtime::datapacks::mutate::install_bytes(
            td.path(),
            "vm-2.0.zip",
            &datapack_zip(b"v2"),
            Some(&prov("v2")),
        )
        .await
        .unwrap();
        assert_eq!(crate::servers_runtime::installed::load(td.path()).len(), 2);

        std::fs::remove_file(td.path().join("datapacks").join("vm-1.0.zip")).unwrap();
        let rows = crate::servers_runtime::datapacks::sidecar::reconcile(td.path());
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].version_id.as_deref(),
            Some("v2"),
            "provenance intact"
        );
    }

    #[tokio::test]
    async fn a_retried_migration_does_not_flip_a_disabled_pack_on() {
        // The new entry's own listed state wins over deriving from the old
        // one: a partial run that already wrote the new name must not have its
        // state re-derived from a stale old entry.
        let td = booted_world_with("vm-1.0.zip", b"v1").await;
        crate::servers_runtime::datapacks::mutate::set_enabled(td.path(), "vm-2.0.zip", false)
            .await
            .unwrap();

        let out = update_one(
            td.path(),
            "vm-1.0.zip",
            "vm-2.0.zip",
            &datapack_zip(b"v2"),
            &prov("v2"),
        )
        .await
        .unwrap();

        assert_eq!(out.was_enabled, Some(false));
        assert_eq!(
            state_of(td.path(), "vm-2.0.zip"),
            Some(WorldPackState::Disabled)
        );
    }
}

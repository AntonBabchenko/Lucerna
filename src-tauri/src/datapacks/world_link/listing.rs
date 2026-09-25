//! The read-only merged per-world view: the world's on-disk entries as the
//! game sees them (`detect::scan`), the registry names that denote them (R2)
//! and the level.dat names, each row with a derived state. No lock.

use std::path::Path;

use crate::datapacks::detect;
use crate::datapacks::{
    level_dat_entry, library_dir_at, presence, registry, state, InstalledDatapack, PackCompat,
    WorldDatapack, WorldDatapackListing,
};
use crate::error::{Error, Result};

use super::world_dirs_checked;

/// List every datapack relevant to one world: the entries in the world's
/// `datapacks/` folder, classified as the game's `PackDetector` does
/// (`detect::scan`), the library's filenames and the names level.dat
/// references (its own `file/` prefix stripped). On-disk entries never merge
/// with each other; a registry name joins the entry it denotes (R2), and a
/// level.dat name joins only an exact or single case-insensitive on-disk match
/// (the display merge, spec §2 N.3). Each row's state uses the engine's exact
/// id, `file/` + the row's own spelling (R1).
///
/// The lists are the ones the game would load (D2): `level.dat`'s, or
/// `level.dat_old`'s when only the backup is left — the game opens the
/// world from it and restores `level.dat`. A folder with neither file is
/// not a world to the game: its listing is empty, and neither the registry
/// nor the folder is read. A presence that cannot be told, or a read error
/// on either file, fails the listing, so the world tab shows its load error
/// rather than rows that guess. So does a `datapacks/` folder that cannot be
/// read (N.1).
///
/// A world folder that does not exist at all fails with `WorldNotFound`,
/// as every writer does: it is not a folder without `level.dat`, and
/// reporting it `Absent` would call a world that is simply gone "not a
/// world".
///
/// `compat` is computed from the pack_format the REGISTRY recorded at
/// install time. Library-vouched zips cost no zip read; any other zip costs
/// one central-directory read (N.1 Cost). A world file with no registry
/// entry (e.g. hand-dropped straight into the world folder, or imported with
/// a world) reports `Unknown` compat deliberately.
pub async fn list_for_world_at(
    instance_root: &Path,
    world: &str,
    expected: Option<u32>,
) -> Result<WorldDatapackListing> {
    let (world_dir, dp_dir) = world_dirs_checked(instance_root, world)?;

    // What the game will load decides the rows (spec §3 L.6). This check
    // comes before the registry is read, because a folder with neither level
    // file is not a world to the game, loads nothing, and has no rows at all.
    let level_dat = presence::of(&world_dir)?;
    let Some((enabled, disabled)) = presence::lists_of(&world_dir, level_dat)? else {
        return Ok(WorldDatapackListing {
            level_dat,
            packs: Vec::new(),
        });
    };

    let registry_entries: Vec<InstalledDatapack> = registry::list(instance_root).await?;
    let lib_dir = library_dir_at(instance_root);
    let dp_owned = dp_dir.clone();
    // `detect::scan` may open unvouched zips: off the executor (N.1).
    let packs = tokio::task::spawn_blocking(move || {
        build_rows(
            &dp_owned,
            &lib_dir,
            &registry_entries,
            &enabled,
            &disabled,
            expected,
        )
    })
    .await
    .map_err(|e| Error::io(dp_dir.display().to_string(), format!("join: {e}")))?
    .map_err(|e| Error::io(dp_dir.display().to_string(), e))?;
    Ok(WorldDatapackListing { level_dat, packs })
}

/// One world tab's rows: one `read_dir` of its `datapacks/`, the registry rows
/// and the two lists already read by the caller.
fn build_rows(
    dp_dir: &Path,
    lib_dir: &Path,
    registry_entries: &[InstalledDatapack],
    enabled: &[String],
    disabled: &[String],
    expected: Option<u32>,
) -> std::io::Result<Vec<WorldDatapack>> {
    let vouch = |name: &str, world_path: &Path| {
        vouched_by_library(registry_entries, lib_dir, name, world_path)
    };
    let on_disk = detect::scan(dp_dir, &vouch)?;
    let disk_names: Vec<String> = on_disk.iter().map(|e| e.name.clone()).collect();
    let registry_names: Vec<String> = registry_entries
        .iter()
        .map(|e| e.filename.clone())
        .collect();
    let level_dat_names: Vec<String> = enabled
        .iter()
        .chain(disabled)
        .filter_map(|n| n.strip_prefix("file/").map(str::to_string))
        .collect();
    let resolves_to = |name: &str| detect::resolve_for_display(dp_dir, name, &disk_names);
    let rows = detect::display_merge(&on_disk, &registry_names, &resolves_to, &level_dat_names);
    Ok(rows
        .into_iter()
        .map(|row| {
            let entry = row.on_disk.map(|i| &on_disk[i]);
            let reg = row
                .joined
                .as_deref()
                .and_then(|n| registry_entries.iter().find(|e| e.filename == n));
            // R1 (N.3): membership is exact, on the engine's id — `file/` + the row's own spelling.
            let id = level_dat_entry(&row.filename);
            let (pack_state, ignored_reason) = state::derive_listed(
                entry.map(|e| &e.presence),
                enabled.contains(&id),
                disabled.contains(&id),
                state::loadable_of(reg, entry),
            );
            WorldDatapack {
                filename: row.filename,
                state: pack_state,
                ignored_reason,
                in_library: reg.is_some(),
                compat: compat_of(reg.and_then(|e| e.pack_format), expected),
            }
        })
        .collect())
}

/// N.1 vouch rule (client only). A registry row has the EXACT filename, and
/// the world entry and `<instance>/datapacks/<name>` agree on `(len,
/// modified)`, read from an open handle on each file. A directory entry's
/// copy of those (`DirEntry::metadata`, or Windows' `fs::metadata` when it
/// falls back to the listing on a sharing violation) can be stale for an NTFS
/// hardlink. Any failure or difference means "not vouched", which costs one
/// central-directory read and never changes the verdict.
pub(crate) fn vouched_by_library(
    rows: &[InstalledDatapack],
    lib_dir: &Path,
    name: &str,
    world_path: &Path,
) -> bool {
    if !rows.iter().any(|r| r.filename == name) {
        return false;
    }
    // Could not open or stat either side: not vouched, so the zip gets the
    // real root check. The restrictive answer; it never changes the verdict.
    let handle_meta = |p: &Path| std::fs::File::open(p).and_then(|f| f.metadata());
    let (Ok(world), Ok(lib)) = (handle_meta(world_path), handle_meta(&lib_dir.join(name))) else {
        return false;
    };
    match (world.modified(), lib.modified()) {
        (Ok(w), Ok(l)) => world.len() == lib.len() && w == l,
        _ => false,
    }
}

/// Compare a pack's own `pack_format` against what the instance's Minecraft
/// expects. `Unknown` when either side is unavailable — an unreadable pack,
/// or (see `compat` module) a client jar that hasn't been installed yet.
///
/// Private: the only call site is [`build_rows`] above, in this same
/// file; nothing else in the crate needs it.
#[must_use]
fn compat_of(pack_format: Option<u32>, expected: Option<u32>) -> PackCompat {
    match (pack_format, expected) {
        (Some(p), Some(e)) if p == e => PackCompat::Compatible,
        (Some(p), Some(e)) => PackCompat::Mismatch {
            pack_format: p,
            expected: e,
        },
        _ => PackCompat::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::detect::{
        test_support::{pack_zip, zip_of},
        IgnoredReason,
    };
    use crate::datapacks::level_dat::test_support::seed;
    use crate::datapacks::presence::LevelDatPresence;
    use crate::datapacks::world_link::set_enabled_in_world_at;
    use crate::datapacks::world_link::test_util::*;
    use crate::datapacks::{level_dat, WorldPackState};

    #[tokio::test]
    async fn list_reports_orphaned_for_a_level_dat_name_with_no_file() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        level_dat::test_support::seed(&wd, &["file/ghost.zip"], &[]);

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "ghost.zip");
        assert_eq!(listed[0].state, WorldPackState::Orphaned);
        assert!(!listed[0].in_library);
    }

    #[tokio::test]
    async fn list_reports_not_added_for_a_library_pack_not_in_the_world() {
        let td = tempfile::tempdir().unwrap();
        game_world(td.path(), "Survival");
        seed_library(td.path(), "vm.zip", 48).await;

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "vm.zip");
        assert_eq!(listed[0].state, WorldPackState::NotAdded);
        assert!(listed[0].in_library);
    }

    /// Engine fact (N.0): the pack's id is `file/` + its exact on-disk
    /// spelling, so level.dat's `file/VeinMiner.zip` does not name the file
    /// `veinminer.zip` — the game drops that stale id (`WARN Missing data
    /// pack`) and auto-adds the file. The display merge (N.3) hides a level.dat
    /// spelling whose only case-insensitive match is one on-disk entry, which
    /// states nothing false because the row's state uses the exact id.
    #[tokio::test]
    async fn a_case_mismatched_name_between_level_dat_and_disk_merges_into_one_row() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/veinminer.zip"), pack_zip()).unwrap();
        level_dat::test_support::seed(&wd, &["file/VeinMiner.zip"], &[]);

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;

        assert_eq!(listed.len(), 1, "one physical pack must be one row");
        assert_eq!(
            listed[0].filename, "veinminer.zip",
            "the on-disk spelling must win over level.dat's"
        );
        assert_eq!(listed[0].state, WorldPackState::Enabled);
    }

    /// Engine fact (N.0): `Disabled` is `List.contains`, exact;
    /// `file/VeinMiner.zip` does not name `file/veinminer.zip`. Inverted from
    /// the pre-batch claim.
    #[tokio::test]
    async fn a_case_drifted_disabled_entry_does_not_keep_the_pack_off() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/veinminer.zip"), pack_zip()).unwrap();
        level_dat::test_support::seed(&wd, &[], &["file/VeinMiner.zip"]);

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "veinminer.zip");
        assert_eq!(
            listed[0].state,
            WorldPackState::Enabled,
            "the game adds file/veinminer.zip from the file, and Disabled holds a different string"
        );
    }

    #[tokio::test]
    async fn a_mismatched_pack_format_reports_mismatch() {
        let td = tempfile::tempdir().unwrap();
        game_world(td.path(), "Survival");
        seed_library(td.path(), "vm.zip", 48).await;

        let listed = list_for_world_at(td.path(), "Survival", Some(10))
            .await
            .unwrap()
            .packs;

        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].compat,
            PackCompat::Mismatch {
                pack_format: 48,
                expected: 10
            }
        );
    }

    #[test]
    fn compat_of_matches_reports_compatible() {
        assert_eq!(compat_of(Some(48), Some(48)), PackCompat::Compatible);
    }

    #[test]
    fn compat_of_missing_either_side_is_unknown() {
        assert_eq!(compat_of(None, Some(48)), PackCompat::Unknown);
        assert_eq!(compat_of(Some(48), None), PackCompat::Unknown);
        assert_eq!(compat_of(None, None), PackCompat::Unknown);
    }

    /// D2 / §3 L.1: the game lists a `saves/` folder as a world only if it
    /// holds `level.dat` or `level.dat_old`, and loads nothing from one that
    /// holds neither. The listing is empty — and it does not even read the
    /// registry, whose reconcile would adopt a hand-dropped library file and
    /// persist `installed-datapacks.json`.
    #[tokio::test]
    async fn a_folder_without_level_dat_lists_no_packs() {
        let td = tempfile::tempdir().unwrap();
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("vm.zip"), datapack_zip(48)).unwrap();
        let wd = world_dir(td.path(), "NotAWorld");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks").join("stray.zip"), datapack_zip(48)).unwrap();

        let listed = list_for_world_at(td.path(), "NotAWorld", None)
            .await
            .unwrap();

        assert_eq!(listed.level_dat, LevelDatPresence::Absent);
        assert!(listed.packs.is_empty(), "{:?}", listed.packs);
        assert!(
            !crate::datapacks::registry_path_at(td.path()).exists(),
            "an absent world must not read (and so reconcile) the registry"
        );
        assert!(
            !wd.join("level.dat").exists(),
            "a listing never writes level.dat"
        );
    }

    /// D2 / §3 L.6: a world that lost `level.dat` but kept `level.dat_old`
    /// is opened by the game from the backup, so its rows are the backup's
    /// lists. Reading the missing `level.dat` as "no lists" reports a pack the
    /// backup keeps disabled as Enabled.
    #[tokio::test]
    async fn an_only_old_world_lists_the_states_level_dat_old_holds() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Restoring");
        level_dat::test_support::seed_old(&wd, &[], &["file/vm.zip"]);
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm.zip"), datapack_zip(48)).unwrap();

        let listed = list_for_world_at(td.path(), "Restoring", None)
            .await
            .unwrap();

        assert_eq!(listed.level_dat, LevelDatPresence::OnlyOld);
        assert_eq!(listed.packs.len(), 1, "{:?}", listed.packs);
        assert_eq!(listed.packs[0].filename, "vm.zip");
        assert_eq!(
            listed.packs[0].state,
            WorldPackState::Disabled,
            "the game loads level.dat_old's lists, where the pack is disabled"
        );
    }

    /// A present world reports `Present`, the one state Lucerna edits.
    #[tokio::test]
    async fn a_played_world_reports_its_level_dat_as_present() {
        let td = tempfile::tempdir().unwrap();
        game_world(td.path(), "Survival");

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap();

        assert_eq!(listed.level_dat, LevelDatPresence::Present);
    }

    /// §3 L.3/L.6: "could not tell" is not "absent". A `level.dat` the game
    /// cannot read either (here a directory) fails the listing with an IO
    /// error, rather than an empty world or a false "quit Minecraft".
    #[tokio::test]
    async fn a_level_dat_that_cannot_be_checked_fails_the_listing() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Odd");
        std::fs::create_dir_all(wd.join("level.dat")).unwrap();

        let err = list_for_world_at(td.path(), "Odd", None).await.unwrap_err();

        assert!(matches!(err, crate::error::Error::Io { .. }), "got {err:?}");
    }

    /// A world folder that is not there at all (deleted while its tab was
    /// open, or a stale name) is not a folder "without level.dat": reporting
    /// it `Absent` would show the "not a world" banner for a world that simply
    /// is gone. It fails the listing with `WorldNotFound`, as every writer does.
    #[tokio::test]
    async fn a_world_folder_that_does_not_exist_is_world_not_found() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(td.path().join(".minecraft").join("saves")).unwrap();

        let err = list_for_world_at(td.path(), "Gone", None)
            .await
            .unwrap_err();

        assert!(
            matches!(&err, crate::error::Error::WorldNotFound { folder_name, .. } if folder_name == "Gone"),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn a_folder_datapack_is_reported_enabled_not_orphaned() {
        let td = tempfile::tempdir().unwrap();
        let wd = game_world(td.path(), "Survival");
        // A hand-installed FOLDER datapack, not a `.zip` — Minecraft loads a
        // directory whose `pack.mcmeta` sits directly inside it (N.0), and so
        // must this listing. Before the fix this reported `file_present:
        // false` (only `.zip`-named files were scanned) and, once level.dat
        // records it, that becomes a false `Orphaned`. Its no-mcmeta twin is
        // `a_folder_without_pack_mcmeta_is_listed_as_ignored`.
        std::fs::create_dir_all(wd.join("datapacks/MyFolderPack/data")).unwrap();
        std::fs::write(wd.join("datapacks/MyFolderPack/pack.mcmeta"), b"{}").unwrap();

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "MyFolderPack");
        assert_eq!(
            listed[0].state,
            WorldPackState::Enabled,
            "present and unlisted in level.dat ⇒ Minecraft auto-enables it"
        );
    }

    fn only_row(listed: &[WorldDatapack]) -> &WorldDatapack {
        assert_eq!(listed.len(), 1, "{listed:?}");
        &listed[0]
    }

    #[tokio::test]
    async fn an_upper_case_zip_is_listed_as_ignored() {
        let td = tempfile::tempdir().unwrap();
        let wd = game_world(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/Pack.ZIP"), pack_zip()).unwrap();
        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;
        let row = only_row(&listed);
        assert_eq!(
            (row.state, row.ignored_reason),
            (
                WorldPackState::Ignored,
                Some(IgnoredReason::ZipExtensionNotLowercase)
            )
        );
    }

    #[tokio::test]
    async fn a_rootless_zip_is_listed_as_ignored() {
        let td = tempfile::tempdir().unwrap();
        let wd = game_world(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(
            wd.join("datapacks/rootless.zip"),
            zip_of(&[("readme.txt", b"x")]),
        )
        .unwrap();
        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;
        assert_eq!(
            only_row(&listed).ignored_reason,
            Some(IgnoredReason::ZipWithoutPackMcmeta)
        );
    }

    #[tokio::test]
    async fn a_nested_folder_pack_is_listed_as_ignored() {
        let td = tempfile::tempdir().unwrap();
        let wd = game_world(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks/Outer/Inner/data")).unwrap();
        std::fs::write(wd.join("datapacks/Outer/Inner/pack.mcmeta"), b"{}").unwrap();
        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;
        assert_eq!(
            only_row(&listed).ignored_reason,
            Some(IgnoredReason::FolderPackNestedInside)
        );
    }

    #[tokio::test]
    async fn an_unreadable_datapacks_dir_is_an_error() {
        let td = tempfile::tempdir().unwrap();
        let wd = game_world(td.path(), "Survival");
        std::fs::write(wd.join("datapacks"), b"a file, not a folder").unwrap();
        assert!(list_for_world_at(td.path(), "Survival", None)
            .await
            .is_err());
    }

    /// §0.5 A13: a same-named world zip that is not the library copy
    /// (other length or mtime) is root-checked, never vouched.
    #[tokio::test]
    async fn a_same_named_world_zip_with_other_len_or_mtime_is_root_checked() {
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let wd = game_world(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm.zip"), zip_of(&[("readme.txt", b"x")])).unwrap();
        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;
        let row = only_row(&listed);
        assert!(row.in_library);
        assert_eq!(
            row.ignored_reason,
            Some(IgnoredReason::ZipWithoutPackMcmeta)
        );
    }

    /// Engine (N.0): `Disabled.contains` is exact, so a drifted
    /// `file/VeinMiner.zip` keeps nothing off, and the toggle round-trips.
    #[tokio::test]
    async fn toggling_a_case_drifted_pack_is_never_stuck() {
        let td = tempfile::tempdir().unwrap();
        let wd = game_world(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/veinminer.zip"), pack_zip()).unwrap();
        seed(&wd, &[], &["file/VeinMiner.zip"]);
        set_enabled_in_world_at(td.path(), "Survival", "veinminer.zip", false)
            .await
            .unwrap();
        set_enabled_in_world_at(td.path(), "Survival", "veinminer.zip", true)
            .await
            .unwrap();
        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;
        assert_eq!(only_row(&listed).state, WorldPackState::Enabled);
    }

    /// Engine (N.0): a directory is a pack only when `pack.mcmeta` is a
    /// regular file directly inside it; otherwise the game ignores it.
    #[tokio::test]
    async fn a_folder_without_pack_mcmeta_is_listed_as_ignored() {
        let td = tempfile::tempdir().unwrap();
        let wd = game_world(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks/MyFolderPack/data")).unwrap();
        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;
        assert_eq!(
            (only_row(&listed).state, only_row(&listed).ignored_reason),
            (
                WorldPackState::Ignored,
                Some(IgnoredReason::FolderWithoutPackMcmeta)
            )
        );
    }

    /// N.1 vouch rule, positive path: a world entry hardlinked to the library
    /// copy is the library's file, so it is never opened (a body the zip crate
    /// rejects still lists as a pack).
    #[tokio::test]
    async fn a_hardlinked_library_copy_is_vouched_and_never_opened() {
        let td = tempfile::tempdir().unwrap();
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("vm.zip"), b"garbage the zip crate rejects").unwrap();
        let wd = game_world(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::hard_link(lib.join("vm.zip"), wd.join("datapacks/vm.zip")).unwrap();
        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;
        let row = only_row(&listed);
        assert_eq!(
            (row.state, row.ignored_reason),
            (WorldPackState::Enabled, None)
        );
    }

    /// N.1 vouch rule, the mtime half: the same name and length with another
    /// modification time is not the library copy, so it is root-checked.
    #[tokio::test]
    async fn a_same_sized_world_zip_with_another_mtime_is_root_checked() {
        let td = tempfile::tempdir().unwrap();
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        let body = b"garbage the zip crate rejects";
        std::fs::write(lib.join("vm.zip"), body).unwrap();
        let wd = game_world(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm.zip"), body).unwrap();
        let earlier = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        std::fs::File::options()
            .write(true)
            .open(wd.join("datapacks/vm.zip"))
            .unwrap()
            .set_modified(earlier)
            .unwrap();
        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap()
            .packs;
        assert_eq!(
            only_row(&listed).ignored_reason,
            Some(IgnoredReason::Unreadable),
            "root-checked, and the zip crate cannot read it"
        );
    }
}

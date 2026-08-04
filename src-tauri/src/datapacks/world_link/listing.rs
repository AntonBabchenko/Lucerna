//! The read-only merged per-world view: the union of the registry, the
//! world's on-disk `datapacks/` entries and its level.dat names, each row
//! with a derived state and a compat verdict. No lock — reads only.

use std::collections::BTreeMap;
use std::path::Path;

use crate::datapacks::{
    level_dat, level_dat_entry, registry, state, InstalledDatapack, PackCompat, WorldDatapack,
};
use crate::error::Result;

use super::{contains_ci, read_level_dat_or_empty, world_dirs};

/// The `.zip` files and directories Minecraft's own `datapacks/` scanner
/// would load out of one world's folder: any directory, regardless of what
/// it's called, plus any file whose name ends `.zip`. Without the directory
/// branch, a hand-installed folder datapack never appears "present" here,
/// which `state::derive` then turns into a false `Orphaned`.
///
/// A directory that cannot be read (missing `datapacks/` folder, a world
/// that has never had a pack added) yields an empty list, never an error —
/// mirrors `read_level_dat_or_empty`'s "absent is a supported state" policy.
pub(crate) async fn list_on_disk_entries(dp_dir: &Path) -> Vec<String> {
    let mut on_disk = Vec::new();
    let Ok(mut rd) = tokio::fs::read_dir(dp_dir).await else {
        return on_disk;
    };
    while let Ok(Some(e)) = rd.next_entry().await {
        let name = e.file_name().to_string_lossy().to_string();
        let is_datapack_entry = match e.file_type().await {
            Ok(ft) if ft.is_dir() => true,
            Ok(_) => name.to_ascii_lowercase().ends_with(".zip"),
            Err(_) => false,
        };
        if is_datapack_entry {
            on_disk.push(name);
        }
    }
    on_disk
}

/// Merge one world's three datapack name sources — the library registry, the
/// files/folders actually present in the world's `datapacks/` folder, and
/// the names level.dat references (its own `file/` prefix already stripped
/// by the caller) — into one deduplicated, sorted list.
///
/// NTFS is case-insensitive: a level.dat entry spelled `file/VeinMiner.zip`
/// and an on-disk file named `veinminer.zip` name the SAME file. Deduping on
/// exact string equality (the bug this replaces) kept both spellings as two
/// separate rows — a phantom `Orphaned` for the level.dat spelling (nothing
/// matched it byte-for-byte) alongside a genuine `Enabled` for the on-disk
/// spelling. One physical pack must never render as two contradictory rows,
/// so dedup keys on a case-folded name instead.
///
/// The kept SPELLING for a case-folded collision prefers, in order: on-disk
/// (that's what the file is actually named), then the registry's, then
/// level.dat's. On-disk wins because [`list_for_world_at`]'s own
/// `file_present` check is a plain, un-folded `on_disk.contains(&filename)`
/// — that only stays correct if, whenever a match exists on disk, the
/// chosen spelling IS the on-disk one.
///
/// Display/merge only. Nothing returned from here is written back to the
/// filesystem or level.dat; every write path keeps using the exact filename
/// its own caller gave it.
fn union_names(registry: &[String], on_disk: &[String], level_dat: &[String]) -> Vec<String> {
    let mut by_key: BTreeMap<String, String> = BTreeMap::new();
    // Insertion order is the priority order, LOWEST first: a later insert
    // for the same case-folded key overwrites the earlier one, so the
    // desired winner (on-disk) has to go last.
    for n in level_dat {
        by_key.insert(n.to_lowercase(), n.clone());
    }
    for n in registry {
        by_key.insert(n.to_lowercase(), n.clone());
    }
    for n in on_disk {
        by_key.insert(n.to_lowercase(), n.clone());
    }
    // `BTreeMap` iterates in key order, and the key IS the case-folded name —
    // already sorted the same way the old `sort_by(|a, b|
    // a.to_lowercase().cmp(&b.to_lowercase()))` produced.
    by_key.into_values().collect()
}

/// List every datapack relevant to one world: the union of the library's
/// filenames, the `.zip` files actually present in the world's `datapacks/`
/// folder, and the names level.dat references (its own `file/` prefix
/// stripped). Sorted case-insensitively by filename, deduplicated
/// case-insensitively too — see [`union_names`] for why.
///
/// `compat` is computed from the pack_format the REGISTRY recorded at
/// install time, never by opening a zip here — listing a world must cost no
/// zip reads. A world file with no registry entry (e.g. hand-dropped
/// straight into the world folder, or imported with a world) reports
/// `Unknown` deliberately, rather than opening every zip on every render.
pub async fn list_for_world_at(
    instance_root: &Path,
    world: &str,
    expected: Option<u32>,
) -> Result<Vec<WorldDatapack>> {
    let (world_dir, dp_dir) = world_dirs(instance_root, world)?;

    let registry_entries: Vec<InstalledDatapack> = registry::list(instance_root).await?;
    let (root, _framing) = read_level_dat_or_empty(&world_dir)?;
    let (enabled, disabled) = level_dat::lists(&root);
    let on_disk = list_on_disk_entries(&dp_dir).await;

    let registry_names: Vec<String> = registry_entries
        .iter()
        .map(|e| e.filename.clone())
        .collect();
    let level_dat_names: Vec<String> = enabled
        .iter()
        .chain(disabled.iter())
        .filter_map(|n| n.strip_prefix("file/").map(str::to_string))
        .collect();
    let names = union_names(&registry_names, &on_disk, &level_dat_names);

    let out = names
        .into_iter()
        .map(|filename| {
            let file_present = on_disk.contains(&filename);
            let entry = level_dat_entry(&filename);
            let in_enabled = contains_ci(&enabled, &entry);
            let in_disabled = contains_ci(&disabled, &entry);
            let pack_state = state::derive(file_present, in_enabled, in_disabled);

            let reg = registry_entries
                .iter()
                .find(|e| e.filename.to_lowercase() == filename.to_lowercase());
            let in_library = reg.is_some();
            let compat = compat_of(reg.and_then(|e| e.pack_format), expected);

            WorldDatapack {
                filename,
                state: pack_state,
                in_library,
                compat,
            }
        })
        .collect();

    Ok(out)
}

/// Compare a pack's own `pack_format` against what the instance's Minecraft
/// expects. `Unknown` when either side is unavailable — an unreadable pack,
/// or (see `compat` module) a client jar that hasn't been installed yet.
///
/// Private: the only call site is [`list_for_world_at`] above, in this same
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
    use crate::datapacks::world_link::test_util::*;
    use crate::datapacks::{level_dat, WorldPackState};
    use fastnbt::Value;
    use std::collections::HashMap;

    #[tokio::test]
    async fn list_reports_orphaned_for_a_level_dat_name_with_no_file() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        std::fs::create_dir_all(&wd).unwrap();
        let mut root = Value::Compound(HashMap::new());
        level_dat::set_enabled(&mut root, "file/ghost.zip", true).unwrap();
        level_dat::write_at(&wd, &root, level_dat::Framing::Gzip)
            .await
            .unwrap();

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "ghost.zip");
        assert_eq!(listed[0].state, WorldPackState::Orphaned);
        assert!(!listed[0].in_library);
    }

    #[tokio::test]
    async fn list_reports_not_added_for_a_library_pack_not_in_the_world() {
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "vm.zip");
        assert_eq!(listed[0].state, WorldPackState::NotAdded);
        assert!(listed[0].in_library);
    }

    /// NTFS is case-insensitive: `veinminer.zip` on disk and level.dat's own
    /// `file/VeinMiner.zip` entry name the SAME file. Before the case-folded
    /// union in `union_names`, exact-string dedup kept both spellings as two
    /// separate rows — a phantom `Orphaned` for the level.dat spelling (no
    /// file matched it byte-for-byte) alongside a genuine `Enabled` for the
    /// on-disk spelling. One physical pack must render as exactly one row.
    #[tokio::test]
    async fn a_case_mismatched_name_between_level_dat_and_disk_merges_into_one_row() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/veinminer.zip"), b"stub").unwrap();
        let mut root = Value::Compound(HashMap::new());
        level_dat::set_enabled(&mut root, "file/VeinMiner.zip", true).unwrap();
        level_dat::write_at(&wd, &root, level_dat::Framing::Gzip)
            .await
            .unwrap();

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap();

        assert_eq!(listed.len(), 1, "one physical pack must be one row");
        assert_eq!(
            listed[0].filename, "veinminer.zip",
            "the on-disk spelling must win over level.dat's"
        );
        assert_eq!(listed[0].state, WorldPackState::Enabled);
    }

    /// Regression for the membership check, not just the union: merging
    /// names case-insensitively is not enough if `in_enabled`/`in_disabled`
    /// still compare exact strings against level.dat's own spelling — that
    /// would report this pack `Enabled` (present and unlisted, per
    /// `state::derive`) when level.dat actually disabled it, which is WORSE
    /// than the phantom-row bug the union fixes, because it silently
    /// re-enables a pack the user turned off.
    #[tokio::test]
    async fn a_case_mismatched_disabled_entry_still_reports_disabled() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/veinminer.zip"), b"stub").unwrap();
        let mut root = Value::Compound(HashMap::new());
        level_dat::set_enabled(&mut root, "file/VeinMiner.zip", false).unwrap();
        level_dat::write_at(&wd, &root, level_dat::Framing::Gzip)
            .await
            .unwrap();

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "veinminer.zip");
        assert_eq!(
            listed[0].state,
            WorldPackState::Disabled,
            "an exact-string membership check would miss level.dat's \
             differently-cased entry and report this Enabled instead"
        );
    }

    /// Direct unit coverage for the three-way spelling collision
    /// `union_names` itself resolves: same case-folded key, three different
    /// spellings, one from each source. On-disk must win regardless of
    /// insertion order because [`list_for_world_at`]'s `file_present` check
    /// is an un-folded `on_disk.contains(&filename)` — only correct if the
    /// chosen spelling equals the on-disk one whenever a match exists there.
    #[test]
    fn union_names_prefers_on_disk_over_registry_and_level_dat() {
        let registry = vec!["VEINMINER.zip".to_string()];
        let on_disk = vec!["veinminer.zip".to_string()];
        let level_dat = vec!["VeinMiner.zip".to_string()];

        let names = union_names(&registry, &on_disk, &level_dat);

        assert_eq!(names, vec!["veinminer.zip".to_string()]);
    }

    #[tokio::test]
    async fn a_mismatched_pack_format_reports_mismatch() {
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;

        let listed = list_for_world_at(td.path(), "Survival", Some(10))
            .await
            .unwrap();

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

    #[tokio::test]
    async fn a_folder_datapack_is_reported_enabled_not_orphaned() {
        let td = tempfile::tempdir().unwrap();
        let wd = world_dir(td.path(), "Survival");
        // A hand-installed FOLDER datapack, not a `.zip` — Minecraft's own
        // datapacks/ scanner loads directories under any name, and so must
        // this listing. Before the fix this reported `file_present: false`
        // (only `.zip`-named files were scanned) and, once level.dat records
        // it, that becomes a false `Orphaned`.
        std::fs::create_dir_all(wd.join("datapacks/MyFolderPack/data")).unwrap();

        let listed = list_for_world_at(td.path(), "Survival", None)
            .await
            .unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "MyFolderPack");
        assert_eq!(
            listed[0].state,
            WorldPackState::Enabled,
            "present and unlisted in level.dat ⇒ Minecraft auto-enables it"
        );
    }
}

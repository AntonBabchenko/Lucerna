//! The union listing: on-disk entries, sidecar rows and level.dat names.

use std::collections::BTreeMap;
use std::path::Path;

use crate::datapacks::{level_dat, state, world_link};
use crate::servers_runtime::installed::ServerInstalledRecord;

use super::{sidecar, ServerDatapackEntry};

/// What Minecraft's own `datapacks/` scanner would load: any directory,
/// regardless of name, plus any file ending `.zip`. Returned as
/// `(name, is_dir)`. An absent or unreadable dir yields an empty list, never
/// an error.
fn on_disk_entries(dp_dir: &Path) -> Vec<(String, bool)> {
    let Ok(rd) = std::fs::read_dir(dp_dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            match e.file_type() {
                Ok(ft) if ft.is_dir() => Some((name, true)),
                Ok(_) if name.to_ascii_lowercase().ends_with(".zip") => Some((name, false)),
                _ => None,
            }
        })
        .collect()
}

/// Merge the three name sources into one deduplicated list, case-folded.
///
/// The kept SPELLING for a collision prefers, in order: on-disk, then the
/// sidecar row's, then `level.dat`'s. On-disk has to win because the
/// `present` flag below is a plain, un-folded comparison against the chosen
/// spelling, while `level.dat` membership goes through `contains_ci`. Get it
/// backwards and a case-drifted pack renders as a phantom ghost row whose
/// removal-as-repair then clears the `level.dat` entry of a PRESENT pack —
/// which, since present-and-unlisted auto-enables, silently re-enables a pack
/// the admin disabled.
///
/// Display/merge only: every write path keeps using the exact filename its
/// own caller gave it.
fn union_names(level_dat_names: &[String], sidecar: &[String], on_disk: &[String]) -> Vec<String> {
    let mut by_key: BTreeMap<String, String> = BTreeMap::new();
    // Insertion order is priority order, LOWEST first — a later insert for the
    // same folded key overwrites the earlier one.
    for n in level_dat_names.iter().chain(sidecar).chain(on_disk) {
        by_key.insert(n.to_lowercase(), n.clone());
    }
    by_key.into_values().collect()
}

/// Every datapack this server's world knows about: the union of the on-disk
/// entries, the provenance sidecar and the `level.dat` name lists, each row
/// with its derived state.
///
/// `world_dir` is `runtime/<level>/`; its `datapacks/` child holds the packs.
///
/// **Not read-only:** this reconciles the provenance sidecar against disk and
/// persists the result (best-effort) via [`sidecar::reconcile`], adopting a
/// hand-dropped `.zip` and pruning a row whose file is gone. The client's
/// `datapacks::registry::list` is deliberately the same shape.
pub fn entries(world_dir: &Path) -> Vec<ServerDatapackEntry> {
    let dp_dir = world_dir.join("datapacks");
    let on_disk = on_disk_entries(&dp_dir);
    let records = sidecar::reconcile(world_dir);

    // ABSENT ⇒ empty lists (a real answer: a never-booted world holds no
    // enabled/disabled state). UNREADABLE ⇒ `None` states, listing intact.
    let lists = world_link::read_level_dat_or_empty(world_dir)
        .ok()
        .map(|(root, _)| level_dat::lists(&root));
    // `file/` is stripped with `filter_map`, which DROPS every entry lacking
    // the prefix — `vanilla` and the feature-flag packs every world carries.
    let strip = |v: &[String]| -> Vec<String> {
        v.iter()
            .filter_map(|n| n.strip_prefix("file/").map(str::to_string))
            .collect()
    };
    let (enabled, disabled) = match &lists {
        Some((e, d)) => (strip(e), strip(d)),
        None => (Vec::new(), Vec::new()),
    };

    let on_disk_names: Vec<String> = on_disk.iter().map(|(n, _)| n.clone()).collect();
    let sidecar_names: Vec<String> = records.iter().map(|r| r.filename.clone()).collect();
    let mut level_dat_names = enabled.clone();
    level_dat_names.extend(disabled.clone());

    let mut out: Vec<ServerDatapackEntry> =
        union_names(&level_dat_names, &sidecar_names, &on_disk_names)
            .into_iter()
            .map(|name| {
                let disk = on_disk.iter().find(|(n, _)| *n == name);
                let present = disk.is_some();
                let is_folder = disk.map(|(_, d)| *d).unwrap_or(false);
                let record = records
                    .iter()
                    .find(|r| r.filename.to_lowercase() == name.to_lowercase())
                    .cloned()
                    .unwrap_or_else(|| ServerInstalledRecord {
                        filename: name.clone(),
                        // A folder or a ghost: nothing to hash. The UI keys rows
                        // on the filename precisely because of this.
                        sha1: String::new(),
                        source: None,
                        project_id: None,
                        version_id: None,
                        name: None,
                        version_number: None,
                        enrich_attempted: false,
                    });
                // `Some` only when level.dat was actually readable — an
                // unreadable one degrades every state to unknown rather than
                // guessing. (An ABSENT one is `Some` with empty lists.)
                let st = lists.is_some().then(|| {
                    state::derive(
                        present,
                        world_link::contains_ci(&enabled, &name),
                        world_link::contains_ci(&disabled, &name),
                    )
                });
                ServerDatapackEntry {
                    record,
                    state: st,
                    present,
                    is_folder,
                }
            })
            .collect();
    out.sort_by(|a, b| {
        a.record
            .filename
            .to_lowercase()
            .cmp(&b.record.filename.to_lowercase())
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::{level_dat, level_dat_entry, WorldPackState};
    use std::io::Write;

    fn datapack_zip() -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(br#"{"pack":{"pack_format":48,"description":"Pack"}}"#)
            .unwrap();
        zw.start_file("data/ns/function/tick.mcfunction", opts)
            .unwrap();
        zw.write_all(b"say hi").unwrap();
        zw.finish().unwrap().into_inner()
    }

    fn world(packs: &[&str]) -> tempfile::TempDir {
        let td = tempfile::tempdir().unwrap();
        let dp = td.path().join("datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        for name in packs {
            std::fs::write(dp.join(name), datapack_zip()).unwrap();
        }
        td
    }

    /// Write a level.dat listing `enabled` / `disabled` by bare filename.
    async fn seed_level_dat(world_dir: &std::path::Path, enabled: &[&str], disabled: &[&str]) {
        let mut root = fastnbt::Value::Compound(std::collections::HashMap::new());
        for n in enabled {
            level_dat::set_enabled(&mut root, &level_dat_entry(n), true).unwrap();
        }
        for n in disabled {
            level_dat::set_enabled(&mut root, &level_dat_entry(n), false).unwrap();
        }
        level_dat::write_at(world_dir, &root, level_dat::Framing::Gzip)
            .await
            .unwrap();
    }

    fn find<'a>(rows: &'a [ServerDatapackEntry], name: &str) -> &'a ServerDatapackEntry {
        rows.iter()
            .find(|r| r.record.filename == name)
            .unwrap_or_else(|| panic!("no row named {name} in {rows:?}"))
    }

    #[tokio::test]
    async fn a_present_and_unlisted_pack_reports_enabled() {
        let td = world(&["fresh.zip"]);
        seed_level_dat(td.path(), &[], &[]).await;
        let rows = entries(td.path());
        assert_eq!(
            find(&rows, "fresh.zip").state,
            Some(WorldPackState::Enabled)
        );
    }

    #[tokio::test]
    async fn a_disabled_pack_reports_disabled() {
        let td = world(&["off.zip"]);
        seed_level_dat(td.path(), &[], &["off.zip"]).await;
        assert_eq!(
            find(&entries(td.path()), "off.zip").state,
            Some(WorldPackState::Disabled)
        );
    }

    #[tokio::test]
    async fn a_folder_pack_is_listed_with_its_real_state_not_orphaned() {
        // Audit #6, and the client's own fixed bug: excluding directories from
        // the on-disk source turns a present folder pack into a phantom
        // Orphaned, whose "repair" would clear a LIVE pack's level.dat entry —
        // and since present-and-unlisted auto-enables, silently re-enable a
        // pack the admin had disabled.
        let td = world(&[]);
        std::fs::create_dir_all(td.path().join("datapacks").join("FolderPack")).unwrap();
        seed_level_dat(td.path(), &[], &["FolderPack"]).await;
        let row = find(&entries(td.path()), "FolderPack").clone();
        assert_eq!(row.state, Some(WorldPackState::Disabled));
        assert!(row.present && row.is_folder);
    }

    #[tokio::test]
    async fn both_ghost_kinds_are_surfaced_as_rows() {
        // Audit #8/#10: an Enabled-list name with no file derives to Orphaned,
        // a Disabled-only one to NotAdded. The client could leave NotAdded
        // unmapped because its dialog lists library packs; this union SURFACES
        // the name, so both must be rows the UI can render and clear.
        let td = world(&[]);
        seed_level_dat(td.path(), &["ghost-on.zip"], &["ghost-off.zip"]).await;
        let rows = entries(td.path());
        assert_eq!(
            find(&rows, "ghost-on.zip").state,
            Some(WorldPackState::Orphaned)
        );
        assert_eq!(
            find(&rows, "ghost-off.zip").state,
            Some(WorldPackState::NotAdded)
        );
        assert!(!find(&rows, "ghost-on.zip").present);
    }

    #[tokio::test]
    async fn built_in_level_dat_entries_are_never_rows() {
        // `vanilla` and feature-flag packs sit in every world's Enabled list
        // without the `file/` prefix. The strip is a filter_map, so they drop
        // out — a refuted audit claim, pinned so nobody "fixes" it back in.
        let td = world(&[]);
        let mut root = fastnbt::Value::Compound(std::collections::HashMap::new());
        level_dat::set_enabled(&mut root, "vanilla", true).unwrap();
        level_dat::write_at(td.path(), &root, level_dat::Framing::Gzip)
            .await
            .unwrap();
        assert!(entries(td.path()).is_empty());
    }

    #[test]
    fn an_absent_level_dat_yields_empty_lists_not_unknown_states() {
        // Audit #7: create-server → add-packs → first-boot is the NORMAL flow,
        // and there is no runtime/<level>/level.dat until the server boots.
        // Bare `read_at` errors on an absent file; this must not degrade every
        // state to unknown.
        let td = world(&["pre-boot.zip"]);
        assert!(!td.path().join("level.dat").exists());
        assert_eq!(
            find(&entries(td.path()), "pre-boot.zip").state,
            Some(WorldPackState::Enabled)
        );
    }

    #[test]
    fn an_unreadable_level_dat_degrades_states_to_unknown_without_failing() {
        let td = world(&["p.zip"]);
        std::fs::write(td.path().join("level.dat"), b"not nbt at all").unwrap();
        let rows = entries(td.path());
        assert_eq!(rows.len(), 1, "the listing still returns");
        assert_eq!(rows[0].state, None);
        assert!(rows[0].present, "presence is still known");
    }

    #[tokio::test]
    async fn a_case_drifted_level_dat_name_is_one_row_spelled_as_on_disk() {
        // Audit #5: the spelling-priority half of the dedup rule. `present` is
        // an exact, un-folded test against the chosen spelling, so on-disk has
        // to win — otherwise a present pack renders as a phantom ghost and its
        // "repair" clears a live entry.
        let td = world(&["veinminer.zip"]);
        seed_level_dat(td.path(), &["VeinMiner.zip"], &[]).await;
        let rows = entries(td.path());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].record.filename, "veinminer.zip");
        assert_eq!(rows[0].state, Some(WorldPackState::Enabled));
    }
}

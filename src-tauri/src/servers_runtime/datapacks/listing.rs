//! The server world's listing: its on-disk entries as the game sees them
//! (`datapacks::detect`), the sidecar rows that denote them (R2) and the
//! level.dat names, each row with its derived state.

use std::path::Path;

use crate::datapacks::detect;
use crate::datapacks::presence::{self, LevelDatPresence};
use crate::datapacks::state;
use crate::error::{Error, Result};
use crate::servers_runtime::installed::ServerInstalledRecord;

use super::{sidecar, ServerDatapackEntry, ServerDatapackListing};

/// Every datapack this server's world knows about: the entries of its
/// `datapacks/` folder, classified as the game's `PackDetector` does
/// (`detect::scan`), the provenance sidecar's rows and the `level.dat` name
/// lists, each row with its derived state. On-disk entries never merge with
/// each other; a sidecar row joins the entry it denotes (R2), and a
/// `level.dat` name joins only an exact or single case-insensitive on-disk
/// match (the display merge, spec §2 N.3). Membership is exact on the
/// engine's id (R1).
///
/// The lists are the ones the server would load: `level.dat`'s,
/// `level.dat_old`'s when only the backup is left, or two empty lists for a
/// world not generated yet. A `level.dat` problem never fails the listing:
/// it degrades the states to unknown, and sets `level_dat: None` when the
/// presence itself could not be told. An entry the game ignores keeps its
/// `Ignored` state and reason either way (§0.5 A4): detection does not depend
/// on `level.dat`.
///
/// `Err` = the world's `datapacks/` could not be read; the pane shows it as
/// its load error (§0.5 A4). A missing folder is no entries, not an error.
///
/// `world_dir` is `runtime/<level>/`; its `datapacks/` child holds the packs.
///
/// **Not read-only:** this reconciles the provenance sidecar against disk and
/// persists the result (best-effort) via [`sidecar::reconcile`], adopting a
/// hand-dropped pack and pruning a row whose file is gone. The client's
/// `datapacks::registry::list` is deliberately the same shape.
pub fn entries(world_dir: &Path) -> Result<ServerDatapackListing> {
    let dp_dir = world_dir.join("datapacks");
    // No vouch on the server (N.1): packs are few, and a sidecar row adopted
    // by name was never validated.
    let on_disk = detect::scan(&dp_dir, &|_, _| false)
        .map_err(|e| Error::io(dp_dir.display().to_string(), e))?;
    let records = sidecar::reconcile(world_dir);

    // What the server would load. Present ⇒ `level.dat`; OnlyOld ⇒
    // `level.dat_old`, which it boots from and restores `level.dat` out of.
    // Absent — a world not generated yet — holds no enabled/disabled state:
    // two empty lists are a real answer (the first boot enables every present
    // pack). A presence that cannot be told, or a file that does not parse,
    // degrades every state to unknown with the listing intact (§0.2 I2).
    // Both failures are logged: the pane itself only shows "unknown".
    let level_dat = match presence::of(world_dir) {
        Ok(p) => Some(p),
        Err(e) => {
            crate::diag!(
                "server datapacks: listing {} with unknown states: could not tell whether it \
                 has a level.dat: {e}",
                world_dir.display()
            );
            None
        }
    };
    let lists = match level_dat {
        Some(LevelDatPresence::Absent) => Some((Vec::new(), Vec::new())),
        Some(p) => match presence::lists_of(world_dir, p) {
            Ok(lists) => lists,
            Err(e) => {
                crate::diag!(
                    "server datapacks: listing {} with unknown states: could not read its data \
                     pack lists: {e}",
                    world_dir.display()
                );
                None
            }
        },
        None => None,
    };
    let rows = build_entries(&on_disk, &records, lists.as_ref(), &|name, names| {
        detect::resolve(&dp_dir, name, names)
    });
    Ok(ServerDatapackListing {
        level_dat,
        entries: rows,
    })
}

/// The rows from what `entries` read: the scanned entries, the reconciled
/// sidecar rows and the two lists (`None` = could not be read). `resolve` is
/// R2 bound to the world's `datapacks/`; a test passes its own to reach
/// `Unknown`, which a real file system rarely produces.
fn build_entries(
    on_disk: &[detect::OnDiskEntry],
    records: &[ServerInstalledRecord],
    lists: Option<&(Vec<String>, Vec<String>)>,
    resolve: &dyn Fn(&str, &[String]) -> detect::Resolved,
) -> Vec<ServerDatapackEntry> {
    // `file/` is stripped with `filter_map`, which DROPS every entry lacking
    // the prefix — `vanilla` and the feature-flag packs every world carries.
    let strip = |v: &[String]| -> Vec<String> {
        v.iter()
            .filter_map(|n| n.strip_prefix("file/").map(str::to_string))
            .collect()
    };
    let (enabled, disabled) = match lists {
        Some((e, d)) => (strip(e), strip(d)),
        None => (Vec::new(), Vec::new()),
    };
    let disk_names: Vec<String> = on_disk.iter().map(|e| e.name.clone()).collect();
    let sidecar_names: Vec<String> = records.iter().map(|r| r.filename.clone()).collect();
    let mut level_dat_names = enabled.clone();
    level_dat_names.extend(disabled.iter().cloned());
    // R2 for every sidecar name. One R2 cannot resolve (a stat error) joins
    // its one candidate entry, whose row then shows an unknown state: never a
    // ghost row for the sidecar name claiming the pack is not in this world
    // while the server may already load it (Fallback discipline Q2).
    let mut joined_to = std::collections::HashMap::new();
    let mut unknown: std::collections::HashSet<String> = std::collections::HashSet::new();
    for name in &sidecar_names {
        match detect::join_name(name, &disk_names, resolve) {
            Some(detect::Joined::Entry(n)) => {
                joined_to.insert(name.clone(), n);
            }
            Some(detect::Joined::Unknown { candidate, cause }) => {
                crate::diag!(
                    "server datapacks: could not tell whether {name} is on disk: {cause}; its row \
                     shows an unknown state"
                );
                if let Some(c) = candidate {
                    unknown.insert(c.clone());
                    joined_to.insert(name.clone(), c);
                }
            }
            None => {}
        }
    }
    let resolves_to = |name: &str| joined_to.get(name).cloned();
    let merged = detect::display_merge(on_disk, &sidecar_names, &resolves_to, &level_dat_names);

    let mut rows: Vec<ServerDatapackEntry> = merged
        .into_iter()
        .map(|row| {
            let disk = row.on_disk.map(|i| &on_disk[i]);
            let record = row
                .joined
                .as_deref()
                .and_then(|n| records.iter().find(|r| r.filename == n))
                .cloned()
                .unwrap_or_else(|| ServerInstalledRecord {
                    filename: row.filename.clone(),
                    // A folder, a non-pack or a ghost: nothing to hash. The UI
                    // keys rows on the filename precisely because of this.
                    sha1: String::new(),
                    source: None,
                    project_id: None,
                    version_id: None,
                    name: None,
                    version_number: None,
                    enrich_attempted: false,
                });
            // R1 (N.3): exact, on the engine's id: the row's own spelling.
            // `None` when the lists could not be read — an unreadable file, or
            // a presence that could not be told. (An ABSENT one — never
            // generated — is `Some` with empty lists.)
            let membership = lists.map(|_| {
                (
                    enabled.contains(&row.filename),
                    disabled.contains(&row.filename),
                )
            });
            // An entry the game ignores is ignored whatever pack it is, so an
            // unknown join does not wipe that (§0.5 A4); any other candidate's
            // state is unknown.
            let ignored_on_disk =
                disk.is_some_and(|d| matches!(d.presence, detect::Presence::Unusable { .. }));
            let (state, ignored_reason) = if unknown.contains(&row.filename) && !ignored_on_disk {
                (None, None)
            } else {
                state::derive(disk.map(|d| &d.presence), membership, None)
            };
            ServerDatapackEntry {
                record,
                state,
                ignored_reason,
                present: disk.is_some(),
                is_folder: disk.is_some_and(|d| d.presence.is_dir()),
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        a.record
            .filename
            .to_lowercase()
            .cmp(&b.record.filename.to_lowercase())
            .then_with(|| a.record.filename.cmp(&b.record.filename))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::detect::{test_support::zip_of, IgnoredReason};
    use crate::datapacks::level_dat::test_support::seed;
    use crate::datapacks::presence::LevelDatPresence;
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

    /// A played world's level.dat (`Enabled` starts with `vanilla`) listing
    /// `enabled` / `disabled` by bare filename.
    async fn seed_level_dat(world_dir: &std::path::Path, enabled: &[&str], disabled: &[&str]) {
        let owned =
            |names: &[&str]| -> Vec<String> { names.iter().map(|n| level_dat_entry(n)).collect() };
        let (en_owned, dis_owned) = (owned(enabled), owned(disabled));
        let en: Vec<&str> = en_owned.iter().map(String::as_str).collect();
        let dis: Vec<&str> = dis_owned.iter().map(String::as_str).collect();
        level_dat::test_support::seed(world_dir, &en, &dis);
    }

    fn find<'a>(rows: &'a [ServerDatapackEntry], name: &str) -> &'a ServerDatapackEntry {
        rows.iter()
            .find(|r| r.record.filename == name)
            .unwrap_or_else(|| panic!("no row named {name} in {rows:?}"))
    }

    fn rows(world_dir: &std::path::Path) -> Vec<ServerDatapackEntry> {
        entries(world_dir).unwrap().entries
    }

    #[tokio::test]
    async fn a_present_and_unlisted_pack_reports_enabled() {
        let td = world(&["fresh.zip"]);
        seed_level_dat(td.path(), &[], &[]).await;
        let rows = rows(td.path());
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
            find(&rows(td.path()), "off.zip").state,
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
        // A folder pack has `pack.mcmeta` directly inside it (N.0); its empty
        // twin is `an_empty_folder_is_ignored_even_when_level_dat_lists_it`.
        let td = world(&[]);
        std::fs::create_dir_all(td.path().join("datapacks").join("FolderPack")).unwrap();
        std::fs::write(td.path().join("datapacks/FolderPack/pack.mcmeta"), b"{}").unwrap();
        seed_level_dat(td.path(), &[], &["FolderPack"]).await;
        let row = find(&rows(td.path()), "FolderPack").clone();
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
        let rows = rows(td.path());
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
        level_dat::test_support::seed(td.path(), &[], &[]);
        assert!(rows(td.path()).is_empty());
    }

    /// §3 L.6: a server boots an only-old world from `level.dat_old` and
    /// restores `level.dat` from it, so the rows carry the backup's states.
    #[test]
    fn an_only_old_world_reports_level_dat_old_states() {
        let td = world(&["vm.zip"]);
        level_dat::test_support::seed_old(td.path(), &[], &["file/vm.zip"]);

        let listing = entries(td.path()).unwrap();

        assert_eq!(listing.level_dat, Some(LevelDatPresence::OnlyOld));
        assert_eq!(
            find(&listing.entries, "vm.zip").state,
            Some(WorldPackState::Disabled),
            "the server boots from level.dat_old, where the pack is disabled"
        );
        assert!(!td.path().join("level.dat").exists());
    }

    /// §0.5 A13 / §0.2 I2: a `level.dat` the server cannot read either (a
    /// directory) is "could not tell": `level_dat: None`, unknown states, the
    /// listing intact.
    #[test]
    fn a_level_dat_that_is_a_directory_gives_unknown_level_dat_and_states() {
        let td = world(&["p.zip"]);
        std::fs::create_dir(td.path().join("level.dat")).unwrap();

        let listing = entries(td.path()).unwrap();

        assert_eq!(listing.level_dat, None, "could not tell is not absent");
        assert_eq!(listing.entries.len(), 1, "the listing still returns");
        assert_eq!(listing.entries[0].state, None);
        assert!(listing.entries[0].present);
    }

    #[test]
    fn an_absent_level_dat_yields_empty_lists_not_unknown_states() {
        // Audit #7: create-server → add-packs → first-boot is the NORMAL flow,
        // and there is no runtime/<level>/level.dat until the server boots.
        // Bare `read_at` errors on an absent file; this must not degrade every
        // state to unknown.
        let td = world(&["pre-boot.zip"]);
        assert!(!td.path().join("level.dat").exists());
        let listing = entries(td.path()).unwrap();
        assert_eq!(listing.level_dat, Some(LevelDatPresence::Absent));
        assert_eq!(
            find(&listing.entries, "pre-boot.zip").state,
            Some(WorldPackState::Enabled)
        );
    }

    #[test]
    fn an_unreadable_level_dat_degrades_states_to_unknown_without_failing() {
        let td = world(&["p.zip"]);
        std::fs::write(td.path().join("level.dat"), b"not nbt at all").unwrap();
        let listing = entries(td.path()).unwrap();
        assert_eq!(
            listing.level_dat,
            Some(LevelDatPresence::Present),
            "the file is there; only its contents are unknown"
        );
        assert_eq!(listing.entries.len(), 1, "the listing still returns");
        assert_eq!(listing.entries[0].state, None);
        assert!(listing.entries[0].present, "presence is still known");
    }

    #[tokio::test]
    async fn a_case_drifted_level_dat_name_is_one_row_spelled_as_on_disk() {
        // Audit #5: the spelling-priority half of the dedup rule. `present` is
        // an exact, un-folded test against the chosen spelling, so on-disk has
        // to win — otherwise a present pack renders as a phantom ghost and its
        // "repair" clears a live entry.
        let td = world(&["veinminer.zip"]);
        seed_level_dat(td.path(), &["VeinMiner.zip"], &[]).await;
        let rows = rows(td.path());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].record.filename, "veinminer.zip");
        assert_eq!(rows[0].state, Some(WorldPackState::Enabled));
    }

    #[test]
    fn an_empty_folder_is_ignored_even_when_level_dat_lists_it() {
        let td = world(&[]);
        std::fs::create_dir_all(td.path().join("datapacks").join("FolderPack")).unwrap();
        seed(td.path(), &[], &["file/FolderPack"]);
        let r = rows(td.path());
        let row = find(&r, "FolderPack");
        assert_eq!(
            (row.state, row.ignored_reason),
            (
                Some(WorldPackState::Ignored),
                Some(IgnoredReason::FolderWithoutPackMcmeta)
            )
        );
        assert!(row.present && row.is_folder);
    }

    /// Engine fact (N.0): `Disabled` is `List.contains`, exact, so a
    /// differently-capitalised entry keeps nothing off.
    #[test]
    fn a_case_drifted_disabled_entry_reports_enabled() {
        let td = world(&["veinminer.zip"]);
        seed(td.path(), &[], &["file/VeinMiner.zip"]);
        let r = rows(td.path());
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].state, Some(WorldPackState::Enabled));
    }

    #[test]
    fn upper_case_and_rootless_zips_are_ignored() {
        let td = world(&["Upper.ZIP"]);
        std::fs::write(
            td.path().join("datapacks/rootless.zip"),
            zip_of(&[("readme.txt", b"x")]),
        )
        .unwrap();
        seed(td.path(), &[], &[]);
        let r = rows(td.path());
        assert_eq!(
            find(&r, "Upper.ZIP").ignored_reason,
            Some(IgnoredReason::ZipExtensionNotLowercase)
        );
        assert_eq!(
            find(&r, "rootless.zip").ignored_reason,
            Some(IgnoredReason::ZipWithoutPackMcmeta)
        );
        assert!(r.iter().all(|e| e.state == Some(WorldPackState::Ignored)));
    }

    /// §0.5 A4.
    #[test]
    fn an_ignored_entry_keeps_its_reason_when_level_dat_is_unreadable() {
        let td = world(&[]);
        std::fs::create_dir_all(td.path().join("datapacks").join("Loose")).unwrap();
        std::fs::write(td.path().join("level.dat"), b"not nbt at all").unwrap();
        let r = rows(td.path());
        assert_eq!(
            (r[0].state, r[0].ignored_reason),
            (
                Some(WorldPackState::Ignored),
                Some(IgnoredReason::FolderWithoutPackMcmeta)
            )
        );
    }

    /// §0.5 A4: a scan error is the pane's load error.
    #[test]
    fn an_unreadable_datapacks_dir_is_a_listing_error() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("datapacks"), b"a file").unwrap();
        assert!(entries(td.path()).is_err());
    }

    /// R2 could not tell whether the sidecar's `vm.zip` is the world's
    /// `VM.zip` (a stat error). The row joins its one candidate with an
    /// unknown state, instead of a ghost `vm.zip` row claiming the pack is
    /// not in this world while the server may already load it.
    #[test]
    fn a_sidecar_name_r2_could_not_resolve_joins_its_candidate_with_an_unknown_state() {
        let on_disk = vec![detect::OnDiskEntry {
            name: "VM.zip".into(),
            presence: detect::Presence::Pack { is_dir: false },
            vouched: false,
        }];
        let records = vec![ServerInstalledRecord {
            filename: "vm.zip".into(),
            sha1: "aa".into(),
            source: None,
            project_id: Some("veinminer".into()),
            version_id: None,
            name: None,
            version_number: None,
            enrich_attempted: false,
        }];
        let lists = (vec!["vanilla".to_string()], Vec::new());
        let could_not_tell = |name: &str, _: &[String]| {
            assert_eq!(name, "vm.zip");
            detect::Resolved::Unknown(std::io::Error::other("stat failed"))
        };
        let rows = build_entries(&on_disk, &records, Some(&lists), &could_not_tell);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].record.project_id.as_deref(), Some("veinminer"));
        assert_eq!((rows[0].state, rows[0].ignored_reason), (None, None));
        assert!(rows[0].present);
    }

    /// §0.5 A4: detection does not depend on which pack an entry is. When R2
    /// cannot tell whether the sidecar's `vm.zip` is the world's `VM.ZIP`, the
    /// game still ignores `VM.ZIP` whatever it is, and the row says why.
    #[test]
    fn an_unknown_join_keeps_the_reason_the_game_ignores_its_candidate() {
        let on_disk = vec![detect::OnDiskEntry {
            name: "VM.ZIP".into(),
            presence: detect::Presence::Unusable {
                is_dir: false,
                reason: IgnoredReason::ZipExtensionNotLowercase,
            },
            vouched: false,
        }];
        let records = vec![ServerInstalledRecord {
            filename: "vm.zip".into(),
            sha1: "aa".into(),
            source: None,
            project_id: Some("veinminer".into()),
            version_id: None,
            name: None,
            version_number: None,
            enrich_attempted: false,
        }];
        let lists = (vec!["vanilla".to_string()], Vec::new());
        let could_not_tell =
            |_: &str, _: &[String]| detect::Resolved::Unknown(std::io::Error::other("stat failed"));
        let rows = build_entries(&on_disk, &records, Some(&lists), &could_not_tell);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(
            (rows[0].state, rows[0].ignored_reason),
            (
                Some(WorldPackState::Ignored),
                Some(IgnoredReason::ZipExtensionNotLowercase)
            )
        );
    }
}

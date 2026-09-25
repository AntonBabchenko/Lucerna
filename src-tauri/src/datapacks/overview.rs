//! The instance-level library view: every datapack Lucerna knows about, with
//! its state in every world.
//!
//! This is the transpose of [`crate::datapacks::world_link::list_for_world_at`].
//! That answers "what does THIS world hold"; this answers "where does THIS pack
//! live". Both classify a world's entries with the same `detect::scan` and
//! derive their per-(pack, world) state from the same pure [`state::derive`],
//! so the two surfaces can never disagree about a pack.
//!
//! Read-only: it takes no lock and writes no `level.dat`. `registry::list` may
//! persist a reconciliation, which is the same write the world-scoped listing
//! already performs and which the game never reads.

use std::collections::HashMap;
use std::path::Path;

use crate::datapacks::detect::{self, IgnoredReason, OnDiskEntry, Presence, Resolved};
use crate::datapacks::format::FormatVersion;
use crate::datapacks::presence::{self, LevelDatPresence};
use crate::datapacks::{
    level_dat_entry, library_dir_at, registry, state, verdict, world_link, DatapackLibraryEntry,
    DatapackLibraryView, DatapackPlacementView, DatapackWorldView, InstalledDatapack, PackCompat,
};
use crate::error::{Error, Result};

/// What one world contributes: its on-disk entries as the game sees them,
/// its `level.dat` presence, and the two lists the game would load.
struct WorldFacts {
    world: String,
    /// `None` when whether the folder holds `level.dat` could not be told.
    level_dat: Option<LevelDatPresence>,
    /// `level.dat`'s lists, or `level.dat_old`'s for an only-old world.
    /// `None` when they could not be read, when the presence could not be
    /// told, or for a folder with neither file — no world, no state.
    lists: Option<(Vec<String>, Vec<String>)>,
    /// `None` = the world's `datapacks/` could not be read (N.1). Every
    /// pack then gets a could-not-tell placement in that world.
    on_disk: Option<Vec<OnDiskEntry>>,
    /// Registry filename → what it denotes on disk (R2). A name that denotes
    /// nothing is not in the map.
    resolved: HashMap<String, Denotes>,
}

/// What a registry name denotes in one world's `datapacks/` (R2).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Denotes {
    /// The on-disk entry, by its own spelling.
    Entry(String),
    /// R2 could not tell (a stat error other than NotFound): the one entry
    /// differing from the name only in case may or may not be this pack. Its
    /// placement is unknown, and the entry is no world-only pack of its own.
    Unknown { candidate: String },
}

impl Denotes {
    fn entry_name(&self) -> &str {
        match self {
            Denotes::Entry(n) | Denotes::Unknown { candidate: n } => n,
        }
    }
}

/// R2 for every registry row against one world's entry names.
fn resolve_rows(
    dp_dir: &Path,
    rows: &[InstalledDatapack],
    names: &[String],
) -> HashMap<String, Denotes> {
    let mut out = HashMap::new();
    for r in rows {
        let denotes = match detect::resolve(dp_dir, &r.filename, names) {
            Resolved::Exact(n) | Resolved::Folded(n) => Denotes::Entry(n),
            Resolved::Absent => continue,
            Resolved::Unknown(e) => {
                crate::diag!(
                    "datapacks library: could not tell whether {} holds {}: {e}; its placement \
                     shows an unknown state",
                    dp_dir.display(),
                    r.filename
                );
                // `resolve` stats only when exactly one entry differs from the
                // name in case, so that entry is the candidate.
                let folded = r.filename.to_lowercase();
                match names.iter().find(|n| n.to_lowercase() == folded) {
                    Some(c) => Denotes::Unknown {
                        candidate: c.clone(),
                    },
                    None => continue,
                }
            }
        };
        out.insert(r.filename.clone(), denotes);
    }
    out
}

/// Whether an on-disk entry of this world is (or may be) a library pack's,
/// so it is not listed again as a pack found only in worlds.
fn claimed_by_registry(facts: &WorldFacts, entry_name: &str) -> bool {
    facts
        .resolved
        .values()
        .any(|d| d.entry_name() == entry_name)
}

/// Whether a world's own pack entries may become rows of their own ("only in
/// worlds"). Only a world the game opens: `Present`, or `OnlyOld`, which the
/// game restores. A folder with neither file is no world (§3 L.6), and one
/// whose presence could not be told may not be one; every world writer
/// refuses both, so a row adopted from them could never be removed (Fallback
/// discipline Q1: could not tell ⟹ the restrictive answer).
fn adopts_world_only_rows(level_dat: Option<LevelDatPresence>) -> bool {
    match level_dat {
        Some(LevelDatPresence::Present | LevelDatPresence::OnlyOld) => true,
        Some(LevelDatPresence::Absent) | None => false,
    }
}

/// Gather every world's facts with ONE presence stat, ONE list read and ONE
/// `read_dir` per world — not one per (pack, world) pair. Synchronous: the
/// scan may open unvouched zips, so the caller runs it in `spawn_blocking`.
///
/// A world whose presence, lists or `datapacks/` folder cannot be read does
/// NOT fail the listing: reading N worlds instead of one multiplies the
/// chance of hitting a locked file (`WorldInUse` is what a running Minecraft
/// produces), and one locked world must not blank the whole screen. Its packs
/// report `state: None` instead, and the folder still appears in `worlds` —
/// with `level_dat: None` when it was the presence that could not be told.
fn gather(instance_root: &Path, rows: &[InstalledDatapack]) -> Vec<WorldFacts> {
    let saves_dir = instance_root.join(".minecraft").join("saves");
    let Ok(rd) = std::fs::read_dir(&saves_dir) else {
        return Vec::new();
    };
    let lib_dir = library_dir_at(instance_root);
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_dir() {
            continue;
        }
        let Some(world) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if crate::worlds::fs::validate_segment(&world).is_err() {
            continue;
        }
        let world_dir = entry.path();
        // Could not tell, or could not read: unknown, never guessed, and
        // logged, because the view itself only shows "unknown". Neither file:
        // not a world to the game, which loads nothing from it, so there is
        // no state to report (`lists_of` answers `None`).
        let level_dat = match presence::of(&world_dir) {
            Ok(p) => Some(p),
            Err(e) => {
                crate::diag!(
                    "datapacks: library view shows {} with no state: could not tell whether it \
                     has a level.dat: {e}",
                    world_dir.display()
                );
                None
            }
        };
        let lists = match level_dat {
            Some(p) => match presence::lists_of(&world_dir, p) {
                Ok(lists) => lists,
                Err(e) => {
                    crate::diag!(
                        "datapacks: library view shows {} with no state: could not read its \
                         data pack lists: {e}",
                        world_dir.display()
                    );
                    None
                }
            },
            None => None,
        };
        let dp_dir = world_dir.join("datapacks");
        let vouch = |name: &str, p: &Path| world_link::vouched_by_library(rows, &lib_dir, name, p);
        let on_disk = match detect::scan(&dp_dir, &vouch) {
            Ok(entries) => Some(entries),
            Err(e) => {
                crate::diag!(
                    "datapacks library: could not read {}: {e}; its placements show an unknown state",
                    dp_dir.display()
                );
                None
            }
        };
        let resolved = match &on_disk {
            Some(entries) => {
                let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
                resolve_rows(&dp_dir, rows, &names)
            }
            None => HashMap::new(),
        };
        out.push(WorldFacts {
            world,
            level_dat,
            lists,
            on_disk,
            resolved,
        });
    }
    out.sort_by(|a, b| a.world.to_lowercase().cmp(&b.world.to_lowercase()));
    out
}

/// Every datapack this instance knows about, with its state in every world.
///
/// `game` is the data-pack format the instance's client jar reports,
/// resolved by the caller (the command layer owns the `AppHandle` needed to
/// find the client jar). Each entry's `compat` is the game's verdict on the
/// library copy's recorded declaration.
pub async fn list_at(
    instance_root: &Path,
    game: Option<FormatVersion>,
) -> Result<DatapackLibraryView> {
    let stored = registry::list_rows(instance_root).await?;
    let rows: Vec<InstalledDatapack> = stored.iter().map(|s| s.pack.clone()).collect();
    let root = instance_root.to_path_buf();
    let rows_for_scan = rows.clone();
    let worlds = tokio::task::spawn_blocking(move || gather(&root, &rows_for_scan))
        .await
        .map_err(|e| Error::io(instance_root.display().to_string(), format!("join: {e}")))?;

    // Library rows, then world-only PACKS: an on-disk pack entry that no
    // registry name resolves to (R2). A pack removed from the library without
    // cascading is gone from the registry but still loading in game; leaving
    // it out would make live content invisible. Keyed exactly (N.3); an entry
    // the game ignores is not a pack, so the world tab shows it and this
    // screen does not (N.1 Q1). Adopted only from a world the game opens
    // (`adopts_world_only_rows`); such a pack still lists every folder that
    // holds it as a placement.
    let mut names: Vec<(String, bool)> = rows.iter().map(|r| (r.filename.clone(), true)).collect();
    for facts in worlds
        .iter()
        .filter(|f| adopts_world_only_rows(f.level_dat))
    {
        for e in facts.on_disk.iter().flatten() {
            if claimed_by_registry(facts, &e.name) || !matches!(e.presence, Presence::Pack { .. }) {
                continue;
            }
            if !names.iter().any(|(n, _)| *n == e.name) {
                names.push((e.name.clone(), false));
            }
        }
    }
    names.sort_by(|a, b| {
        a.0.to_lowercase()
            .cmp(&b.0.to_lowercase())
            .then_with(|| a.0.cmp(&b.0))
    });

    let entries = names
        .into_iter()
        .map(|(filename, in_registry)| {
            let row = if in_registry {
                rows.iter().find(|r| r.filename == filename)
            } else {
                None
            };
            // The library row IS the library copy: its recorded declaration
            // speaks for it (§0.5 A1). A world-only pack has none: Unknown.
            let compat = verdict::verdict(
                row.and_then(|r| registry::mcmeta_of(&stored, &r.filename)),
                game,
            );
            let placements = worlds
                .iter()
                .filter_map(|f| placement_in(f, &filename, row, &compat))
                .collect();
            DatapackLibraryEntry {
                in_library: row.is_some(),
                compat,
                pack: row.cloned().unwrap_or_else(|| unlisted(&filename)),
                placements,
            }
        })
        .collect();

    Ok(DatapackLibraryView {
        entries,
        worlds: worlds
            .iter()
            .map(|f| DatapackWorldView {
                world: f.world.clone(),
                level_dat: f.level_dat,
            })
            .collect(),
    })
}

/// This pack's placement in one world, or `None` when the world does not
/// reference it — a world that has never seen the pack contributes nothing,
/// and listing every world against every pack would turn the expander into
/// noise. The on-disk entry is the one a registry name resolves to (R2), or
/// the entry with exactly this name for a world-only pack. Membership is
/// exact (R1) on the engine's id: the entry's own spelling when there is
/// one — an id whose case drifted from the file names nothing the game loads.
///
/// "Could not tell" is never "not in this world" (Fallback discipline Q2):
/// a world whose `datapacks/` could not be read gives every pack a
/// could-not-tell placement (it may hold it unlisted, auto-enabled) — whether
/// or not its lists could be read, and in a folder with no level file too,
/// where the cascade still unlinks files (§0.5 A3) — and so does a registry
/// name R2 could not resolve. See [`could_not_tell`] for its shape.
///
/// `library` is the library copy's verdict; it decides whether the game loads
/// this world's entry only when the scan vouched that entry as the library's
/// bytes (`state::loadable_of`, §0.5 A1).
fn placement_in(
    f: &WorldFacts,
    filename: &str,
    row: Option<&InstalledDatapack>,
    library: &PackCompat,
) -> Option<DatapackPlacementView> {
    let Some(entries) = f.on_disk.as_ref() else {
        return Some(could_not_tell(f));
    };
    let on_disk_name = match row {
        Some(_) => match f.resolved.get(filename) {
            Some(Denotes::Entry(n)) => Some(n.as_str()),
            Some(Denotes::Unknown { .. }) => return Some(could_not_tell(f)),
            None => None,
        },
        None => Some(filename),
    };
    let entry = on_disk_name.and_then(|n| entries.iter().find(|e| e.name == n));
    let id = level_dat_entry(entry.map_or(filename, |e| e.name.as_str()));
    let listed = f
        .lists
        .as_ref()
        .map(|(en, dis)| (en.contains(&id), dis.contains(&id)));
    if entry.is_none() && !listed.is_some_and(|(e, d)| e || d) {
        return None;
    }
    let (state, ignored_reason) = state::derive(
        entry.map(|e| &e.presence),
        listed,
        state::loadable_of(library, entry),
    );
    Some(DatapackPlacementView {
        world: f.world.clone(),
        state,
        ignored_reason,
        level_dat: f.level_dat,
    })
}

/// A placement Lucerna could not check: its entry, or the world's whole
/// `datapacks/`, could not be read or resolved. `state: None` with the reason
/// an unreadable entry carries, `Unreadable`, and the world's real presence.
/// The reason is what tells it apart from a folder with no level file, whose
/// `state: None` is a fact (no list to hold a state), so the removal dialog
/// lists it under "couldn't check". The one place `ignored_reason` is set
/// without `state == Ignored`: an `Unreadable` entry the scan saw is `Ignored`
/// (`state::derive`), but here there may be no entry to ignore, and the game
/// may well load the pack.
fn could_not_tell(f: &WorldFacts) -> DatapackPlacementView {
    DatapackPlacementView {
        world: f.world.clone(),
        state: None,
        ignored_reason: Some(IgnoredReason::Unreadable),
        level_dat: f.level_dat,
    }
}

/// A placeholder row for a pack that exists only in worlds. Everything the
/// registry would have recorded is genuinely unknown — the sha1 and size would
/// have to be read from a world's own copy, and its provenance is gone for
/// good — so nothing here is fabricated beyond the filename we can see.
fn unlisted(filename: &str) -> InstalledDatapack {
    InstalledDatapack {
        filename: filename.to_string(),
        sha1: String::new(),
        size_bytes: 0.0,
        name: filename.trim_end_matches(".zip").to_string(),
        source: None,
        project_id: None,
        version_id: None,
        version_number: None,
        installed_at: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::detect::{test_support::zip_of, IgnoredReason};
    use crate::datapacks::format::samples;
    use crate::datapacks::world_link::test_util::{game_world, seed_library_with_mcmeta};
    use crate::datapacks::{library, world_link, WorldPackState};
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn datapack_zip(pack_format: u32) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(
            format!(r#"{{"pack":{{"pack_format":{pack_format},"description":"Test pack"}}}}"#)
                .as_bytes(),
        )
        .unwrap();
        zw.start_file("data/x/function/a.mcfunction", opts).unwrap();
        zw.write_all(b"say hi").unwrap();
        zw.finish().unwrap().into_inner()
    }

    async fn seed(root: &Path, filename: &str, pack_format: u32) {
        library::install_named_at(root, filename, &datapack_zip(pack_format), None)
            .await
            .unwrap();
    }

    fn make_world(root: &Path, world: &str) {
        let wd = crate::datapacks::world_link::test_util::game_world(root, world);
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
    }

    fn entry_for<'a>(view: &'a DatapackLibraryView, filename: &str) -> &'a DatapackLibraryEntry {
        view.entries
            .iter()
            .find(|e| e.pack.filename == filename)
            .unwrap_or_else(|| panic!("{filename} missing from {:?}", view.entries.len()))
    }

    #[tokio::test]
    async fn transposes_one_pack_across_three_worlds() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        seed(td.path(), "vm.zip", 48).await;
        for w in ["Alpha", "Beta", "Gamma"] {
            make_world(td.path(), w);
        }
        world_link::add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap();
        world_link::add_to_world_at(td.path(), "Beta", "vm.zip")
            .await
            .unwrap();
        world_link::set_enabled_in_world_at(td.path(), "Beta", "vm.zip", false)
            .await
            .unwrap();
        // Gamma never receives it.

        let view = list_at(td.path(), Some(FormatVersion::new(48, 0)))
            .await
            .unwrap();
        let e = entry_for(&view, "vm.zip");

        assert!(e.in_library);
        assert_eq!(e.compat, PackCompat::Compatible);
        assert_eq!(
            e.placements,
            vec![
                DatapackPlacementView {
                    world: "Alpha".into(),
                    state: Some(WorldPackState::Enabled),
                    ignored_reason: None,
                    level_dat: Some(LevelDatPresence::Present),
                },
                DatapackPlacementView {
                    world: "Beta".into(),
                    state: Some(WorldPackState::Disabled),
                    ignored_reason: None,
                    level_dat: Some(LevelDatPresence::Present),
                },
            ],
            "a world that never received the pack contributes no placement"
        );
    }

    #[tokio::test]
    async fn a_pack_in_no_world_has_empty_placements() {
        let td = tempfile::tempdir().unwrap();
        seed(td.path(), "vm.zip", 48).await;
        make_world(td.path(), "Alpha");

        let view = list_at(td.path(), None).await.unwrap();
        let e = entry_for(&view, "vm.zip");

        assert!(e.in_library);
        assert!(
            e.placements.is_empty(),
            "this is the state the whole library screen exists to surface: {:?}",
            e.placements
        );
        assert_eq!(
            e.compat,
            PackCompat::Unknown,
            "no expected format was given"
        );
    }

    #[tokio::test]
    async fn a_pack_only_in_worlds_is_still_listed() {
        // Reachable through a non-cascading removal. The pack keeps LOADING in
        // game, so leaving it out of the listing would hide live content.
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        seed(td.path(), "vm.zip", 48).await;
        make_world(td.path(), "Alpha");
        world_link::add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap();
        library::remove_at(td.path(), "vm.zip").await.unwrap();

        let view = list_at(td.path(), None).await.unwrap();
        let e = entry_for(&view, "vm.zip");

        assert!(!e.in_library);
        assert_eq!(
            e.placements,
            vec![DatapackPlacementView {
                world: "Alpha".into(),
                state: Some(WorldPackState::Enabled),
                ignored_reason: None,
                level_dat: Some(LevelDatPresence::Present),
            }]
        );
    }

    /// §3 L.6: the game loads nothing from a `saves/` folder with neither
    /// `level.dat` nor `level.dat_old`. A pack file sitting there has no state;
    /// reading the folder as "two empty lists" made it a false Enabled.
    #[tokio::test]
    async fn a_pack_in_a_folder_without_level_dat_has_no_state() {
        let td = tempfile::tempdir().unwrap();
        seed(td.path(), "vm.zip", 48).await;
        let dp = td.path().join(".minecraft/saves/Loose/datapacks");
        std::fs::create_dir_all(&dp).unwrap();
        std::fs::write(dp.join("vm.zip"), datapack_zip(48)).unwrap();

        let view = list_at(td.path(), None).await.unwrap();

        assert_eq!(
            entry_for(&view, "vm.zip").placements,
            vec![DatapackPlacementView {
                world: "Loose".into(),
                state: None,
                ignored_reason: None,
                level_dat: Some(LevelDatPresence::Absent),
            }],
            "the game loads nothing from this folder, so no state is claimed"
        );
    }

    /// §3 L.6 / §0.5 A2: an only-old world's placement keeps the state
    /// `level.dat_old` holds — the one the game will load — and carries the
    /// presence that explains why it cannot be changed.
    /// A pack found only in worlds is adopted as a row of its own only from a
    /// world the game opens (`Present`, or `OnlyOld`, which it restores). A
    /// folder with no level file is no world, and one whose presence could not
    /// be told may not be either: every world writer refuses both, so a row
    /// adopted from them offered a removal that could never succeed. A folder
    /// still shows as a placement of a pack a real world adopted.
    #[tokio::test]
    async fn world_only_rows_are_adopted_only_from_worlds_the_game_opens() {
        let td = tempfile::tempdir().unwrap();
        let saves = td.path().join(".minecraft/saves");
        let drop = |world: &str, file: &str| {
            let dp = saves.join(world).join("datapacks");
            std::fs::create_dir_all(&dp).unwrap();
            std::fs::write(dp.join(file), datapack_zip(48)).unwrap();
        };
        game_world(td.path(), "Alpha");
        drop("Alpha", "shared.zip");
        crate::datapacks::level_dat::test_support::seed_old(&saves.join("Restoring"), &[], &[]);
        drop("Restoring", "old.zip");
        drop("Loose", "shared.zip");
        drop("Loose", "stray.zip");
        std::fs::create_dir_all(saves.join("Odd").join("level.dat")).unwrap();
        drop("Odd", "odd.zip");

        let view = list_at(td.path(), None).await.unwrap();

        let names: Vec<&str> = view
            .entries
            .iter()
            .map(|e| e.pack.filename.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["old.zip", "shared.zip"],
            "no row adopted from a folder with no level file or an untellable one"
        );
        let shared: Vec<(&str, Option<LevelDatPresence>)> = entry_for(&view, "shared.zip")
            .placements
            .iter()
            .map(|p| (p.world.as_str(), p.level_dat))
            .collect();
        assert_eq!(
            shared,
            vec![
                ("Alpha", Some(LevelDatPresence::Present)),
                ("Loose", Some(LevelDatPresence::Absent)),
            ]
        );
    }

    #[tokio::test]
    async fn an_only_old_world_reports_level_dat_old_state() {
        let td = tempfile::tempdir().unwrap();
        seed(td.path(), "vm.zip", 48).await;
        let wd = td.path().join(".minecraft/saves/Restoring");
        crate::datapacks::level_dat::test_support::seed_old(&wd, &[], &["file/vm.zip"]);
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/vm.zip"), datapack_zip(48)).unwrap();

        let view = list_at(td.path(), None).await.unwrap();

        assert_eq!(
            entry_for(&view, "vm.zip").placements,
            vec![DatapackPlacementView {
                world: "Restoring".into(),
                state: Some(WorldPackState::Disabled),
                ignored_reason: None,
                level_dat: Some(LevelDatPresence::OnlyOld),
            }]
        );
    }

    #[tokio::test]
    async fn an_unreadable_level_dat_reports_unknown_without_blanking_the_listing() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        seed(td.path(), "vm.zip", 48).await;
        make_world(td.path(), "Alpha");
        make_world(td.path(), "Broken");
        world_link::add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap();
        world_link::add_to_world_at(td.path(), "Broken", "vm.zip")
            .await
            .unwrap();
        // Corrupt Broken's level.dat: present but unparseable. Its presence is
        // `Present` and its state unknown — unlike a folder with no level.dat,
        // which is `Absent` with no state
        // (`a_pack_in_a_folder_without_level_dat_has_no_state`).
        let broken = td
            .path()
            .join(".minecraft")
            .join("saves")
            .join("Broken")
            .join("level.dat");
        std::fs::write(&broken, b"not nbt at all").unwrap();

        let view = list_at(td.path(), None).await.unwrap();
        let e = entry_for(&view, "vm.zip");

        assert_eq!(
            e.placements.len(),
            2,
            "one bad world must not drop the other"
        );
        let alpha = e.placements.iter().find(|p| p.world == "Alpha").unwrap();
        assert_eq!(alpha.state, Some(WorldPackState::Enabled));
        let bad = e.placements.iter().find(|p| p.world == "Broken").unwrap();
        assert_eq!(bad.state, None, "unknown, not guessed");
        assert_eq!(
            bad.level_dat,
            Some(LevelDatPresence::Present),
            "the file is there; only its contents are unknown"
        );
    }

    /// §0.5 A13 / §3 L.6: `worlds` names every folder `gather` saw — with or
    /// without the pack — each with its presence; one whose presence cannot be
    /// told is `None`, not `Absent`.
    #[tokio::test]
    async fn worlds_lists_every_world_folder_including_ones_without_the_pack() {
        let td = tempfile::tempdir().unwrap();
        let saves = td.path().join(".minecraft").join("saves");
        world_link::test_util::game_world(td.path(), "alpha");
        crate::datapacks::level_dat::test_support::seed_old(&saves.join("Beta"), &[], &[]);
        std::fs::create_dir_all(saves.join("Loose")).unwrap();
        std::fs::create_dir_all(saves.join("Odd").join("level.dat")).unwrap();

        let view = list_at(td.path(), None).await.unwrap();

        assert_eq!(
            view.worlds,
            vec![
                DatapackWorldView {
                    world: "alpha".into(),
                    level_dat: Some(LevelDatPresence::Present),
                },
                DatapackWorldView {
                    world: "Beta".into(),
                    level_dat: Some(LevelDatPresence::OnlyOld),
                },
                DatapackWorldView {
                    world: "Loose".into(),
                    level_dat: Some(LevelDatPresence::Absent),
                },
                DatapackWorldView {
                    world: "Odd".into(),
                    level_dat: None,
                },
            ],
            "one row per folder, sorted case-insensitively, pack or no pack"
        );
        assert!(view.entries.is_empty());
    }

    #[tokio::test]
    async fn a_pack_made_for_an_older_format_is_reported_once_on_the_row() {
        // Was `a_mismatched_pack_format_is_reported_once_on_the_row`: the game
        // labels a format below its own "made for an older version" and still
        // loads it (era B).
        let td = tempfile::tempdir().unwrap();
        seed(td.path(), "vm.zip", 48).await;

        let view = list_at(td.path(), Some(FormatVersion::new(57, 0)))
            .await
            .unwrap();

        assert_eq!(
            entry_for(&view, "vm.zip").compat,
            PackCompat::TooOld {
                made_for: "48".into(),
                game: "57".into()
            }
        );
    }

    #[tokio::test]
    async fn a_wont_load_placement_is_ignored() {
        // The library copy declares no pack_format; 1.21.1 skips it. Its
        // vouched link in Alpha is therefore Ignored(NotLoadable) (§0.5 A1).
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library_with_mcmeta(td.path(), "nullscape.zip", samples::NULLSCAPE).await;
        game_world(td.path(), "Alpha");
        world_link::add_to_world_at(td.path(), "Alpha", "nullscape.zip")
            .await
            .unwrap();

        let view = list_at(td.path(), Some(FormatVersion::new(48, 0)))
            .await
            .unwrap();
        let e = entry_for(&view, "nullscape.zip");
        assert_eq!(
            e.compat,
            PackCompat::WontLoad {
                reason: crate::datapacks::WontLoadReason::NoPackFormat
            }
        );
        let alpha = e.placements.iter().find(|p| p.world == "Alpha").unwrap();
        assert_eq!(alpha.state, Some(WorldPackState::Ignored));
        assert_eq!(alpha.ignored_reason, Some(IgnoredReason::NotLoadable));
    }

    #[tokio::test]
    async fn a_hand_dropped_copy_is_never_labelled_not_loadable() {
        // §0.5 A1: the library copy's WontLoad speaks only for the library's
        // own bytes. A same-named world zip with other bytes is not vouched,
        // so its placement keeps the state the game gives a loadable pack.
        let td = tempfile::tempdir().unwrap();
        seed_library_with_mcmeta(td.path(), "vm.zip", samples::NULLSCAPE).await;
        let wd = game_world(td.path(), "Alpha");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(
            wd.join("datapacks/vm.zip"),
            samples::zip_with_mcmeta(samples::DAGGER),
        )
        .unwrap();

        let view = list_at(td.path(), Some(FormatVersion::new(48, 0)))
            .await
            .unwrap();
        let alpha = entry_for(&view, "vm.zip")
            .placements
            .iter()
            .find(|p| p.world == "Alpha")
            .unwrap()
            .clone();
        assert_eq!(alpha.state, Some(WorldPackState::Enabled));
        assert_eq!(alpha.ignored_reason, None);
    }

    #[tokio::test]
    async fn a_supported_formats_range_covering_the_game_is_compatible() {
        // bettercaps: pack_format 48, supported_formats 34–48. On 1.20.6
        // (data 41) the game takes the range — compatible (§1 C3 era B).
        let td = tempfile::tempdir().unwrap();
        seed_library_with_mcmeta(td.path(), "bettercaps.zip", samples::BETTERCAPS).await;
        let view = list_at(td.path(), Some(FormatVersion::new(41, 0)))
            .await
            .unwrap();
        assert_eq!(
            entry_for(&view, "bettercaps.zip").compat,
            PackCompat::Compatible
        );
    }

    #[tokio::test]
    async fn a_placement_in_both_lists_is_enabled() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        seed(td.path(), "vm.zip", 48).await;
        let wd = game_world(td.path(), "Alpha");
        world_link::add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap();
        crate::datapacks::level_dat::test_support::seed(&wd, &["file/vm.zip"], &["file/vm.zip"]);
        let view = list_at(td.path(), None).await.unwrap();
        let p = &entry_for(&view, "vm.zip").placements;
        assert_eq!(p.len(), 1);
        assert_eq!(
            p[0].state,
            Some(WorldPackState::Enabled),
            "an available Enabled id stays selected (N.0)"
        );
    }

    #[tokio::test]
    async fn a_case_drifted_disabled_placement_is_enabled() {
        let _lock = crate::test_env_lock();
        let td = tempfile::tempdir().unwrap();
        seed(td.path(), "vm.zip", 48).await;
        let wd = game_world(td.path(), "Alpha");
        world_link::add_to_world_at(td.path(), "Alpha", "vm.zip")
            .await
            .unwrap();
        crate::datapacks::level_dat::test_support::seed(&wd, &[], &["file/VM.zip"]);
        let view = list_at(td.path(), None).await.unwrap();
        assert_eq!(
            entry_for(&view, "vm.zip").placements[0].state,
            Some(WorldPackState::Enabled)
        );
    }

    /// N.1 / Q1: a non-pack world entry is not an "only in worlds" pack.
    #[tokio::test]
    async fn an_entry_the_game_ignores_is_not_an_only_in_worlds_pack() {
        let td = tempfile::tempdir().unwrap();
        let wd = game_world(td.path(), "Alpha");
        std::fs::create_dir_all(wd.join("datapacks/Loose/data")).unwrap();
        std::fs::write(
            wd.join("datapacks/junk.zip"),
            zip_of(&[("readme.txt", b"x")]),
        )
        .unwrap();
        assert!(list_at(td.path(), None).await.unwrap().entries.is_empty());
    }

    /// N.5: a legacy `X.ZIP` library row stays listed; its world link shows as Ignored.
    #[tokio::test]
    async fn a_legacy_upper_case_link_is_an_ignored_placement() {
        let td = tempfile::tempdir().unwrap();
        let bytes = datapack_zip(48);
        let lib = crate::datapacks::library_dir_at(td.path());
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("Legacy.ZIP"), &bytes).unwrap();
        let wd = game_world(td.path(), "Alpha");
        std::fs::create_dir_all(wd.join("datapacks")).unwrap();
        std::fs::write(wd.join("datapacks/Legacy.ZIP"), &bytes).unwrap();
        let view = list_at(td.path(), None).await.unwrap();
        let p = &entry_for(&view, "Legacy.ZIP").placements;
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(
            (p[0].state, p[0].ignored_reason),
            (
                Some(WorldPackState::Ignored),
                Some(IgnoredReason::ZipExtensionNotLowercase)
            )
        );
    }

    /// Fallback Q1/Q2: a world whose `datapacks/` cannot be read may hold any
    /// library pack, unlisted and auto-enabled. Every library pack gets an
    /// unknown placement there, never none (which the picker reads as "not in
    /// this world, add it").
    #[tokio::test]
    async fn an_unreadable_datapacks_folder_gives_every_library_pack_an_unknown_placement() {
        let td = tempfile::tempdir().unwrap();
        seed(td.path(), "vm.zip", 48).await;
        let wd = game_world(td.path(), "Alpha");
        std::fs::write(wd.join("datapacks"), b"a file, not a folder").unwrap();

        let view = list_at(td.path(), None).await.unwrap();

        assert_eq!(
            entry_for(&view, "vm.zip").placements,
            vec![DatapackPlacementView {
                world: "Alpha".into(),
                state: None,
                ignored_reason: Some(IgnoredReason::Unreadable),
                level_dat: Some(LevelDatPresence::Present),
            }]
        );
    }

    fn facts_with(resolved: HashMap<String, Denotes>, on_disk: Vec<OnDiskEntry>) -> WorldFacts {
        WorldFacts {
            world: "Alpha".into(),
            level_dat: Some(LevelDatPresence::Present),
            lists: Some((vec!["vanilla".into()], Vec::new())),
            on_disk: Some(on_disk),
            resolved,
        }
    }

    /// R2 could not tell whether `VM.zip` is the library's `vm.zip` (a stat
    /// error): the placement is unknown, not NotAdded/Orphaned, and the entry
    /// is not listed again as a pack found only in worlds.
    #[test]
    fn a_name_r2_could_not_resolve_has_an_unknown_placement_and_no_duplicate_row() {
        let entry = OnDiskEntry {
            name: "VM.zip".into(),
            presence: Presence::Pack { is_dir: false },
            vouched: false,
        };
        let resolved = HashMap::from([(
            "vm.zip".to_string(),
            Denotes::Unknown {
                candidate: "VM.zip".into(),
            },
        )]);
        let facts = facts_with(resolved.clone(), vec![entry.clone()]);
        let p = placement_in(
            &facts,
            "vm.zip",
            Some(&unlisted("vm.zip")),
            &PackCompat::Unknown,
        )
        .unwrap();
        assert_eq!(
            (p.state, p.ignored_reason, p.level_dat),
            (
                None,
                Some(IgnoredReason::Unreadable),
                Some(LevelDatPresence::Present)
            ),
            "could not tell, marked as such, with the world's own presence"
        );
        assert!(claimed_by_registry(&facts, "VM.zip"));

        // In a folder with no level file, `state: None` alone reads as "no
        // world to hold a state"; the mark is what says "could not tell".
        let loose = WorldFacts {
            level_dat: Some(LevelDatPresence::Absent),
            lists: None,
            ..facts_with(resolved, vec![entry])
        };
        let p = placement_in(
            &loose,
            "vm.zip",
            Some(&unlisted("vm.zip")),
            &PackCompat::Unknown,
        )
        .unwrap();
        assert_eq!(
            (p.state, p.ignored_reason, p.level_dat),
            (
                None,
                Some(IgnoredReason::Unreadable),
                Some(LevelDatPresence::Absent)
            )
        );
    }

    /// Fallback Q2: a world whose `datapacks/` AND lists could not be read may
    /// hold any library pack. It gets a could-not-tell placement, never none
    /// (which the removal dialog reads as "not in this world"): the cascade
    /// fails such a world and keeps the library copy. So does a folder with
    /// no level file whose `datapacks/` could not be read: the cascade unlinks
    /// files there (§0.5 A3), and cannot check it either.
    #[tokio::test]
    async fn a_world_whose_datapacks_and_lists_cannot_be_read_is_could_not_tell() {
        let td = tempfile::tempdir().unwrap();
        seed(td.path(), "vm.zip", 48).await;
        let wd = game_world(td.path(), "Sealed");
        std::fs::write(wd.join("level.dat"), b"not nbt at all").unwrap();
        std::fs::write(wd.join("datapacks"), b"a file, not a folder").unwrap();
        let loose = td.path().join(".minecraft/saves/Loose");
        std::fs::create_dir_all(&loose).unwrap();
        std::fs::write(loose.join("datapacks"), b"a file, not a folder").unwrap();

        let view = list_at(td.path(), None).await.unwrap();

        assert_eq!(
            entry_for(&view, "vm.zip").placements,
            vec![
                DatapackPlacementView {
                    world: "Loose".into(),
                    state: None,
                    ignored_reason: Some(IgnoredReason::Unreadable),
                    level_dat: Some(LevelDatPresence::Absent),
                },
                DatapackPlacementView {
                    world: "Sealed".into(),
                    state: None,
                    ignored_reason: Some(IgnoredReason::Unreadable),
                    level_dat: Some(LevelDatPresence::Present),
                },
            ]
        );
    }
}

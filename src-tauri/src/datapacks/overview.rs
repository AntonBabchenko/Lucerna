//! The instance-level library view: every datapack Lucerna knows about, with
//! its state in every world.
//!
//! This is the transpose of [`crate::datapacks::world_link::list_for_world_at`].
//! That answers "what does THIS world hold"; this answers "where does THIS pack
//! live". Both derive their per-(pack, world) state from the same pure
//! [`state::derive`], so the two surfaces can never disagree about a pack.
//!
//! Read-only: it takes no lock and writes no `level.dat`. `registry::list` may
//! persist a reconciliation, which is the same write the world-scoped listing
//! already performs and which the game never reads.

use std::collections::BTreeMap;
use std::path::Path;

use crate::datapacks::{
    level_dat, level_dat_entry, registry, state, world_link, DatapackLibraryEntry,
    DatapackLibraryView, DatapackPlacementView, InstalledDatapack, PackCompat,
};
use crate::error::Result;

/// What one world contributes: the names physically present in its
/// `datapacks/` folder, plus its two level.dat lists. `lists` is `None` when
/// level.dat could not be read at all.
struct WorldFacts {
    world: String,
    on_disk: Vec<String>,
    lists: Option<(Vec<String>, Vec<String>)>,
}

/// Gather every world's facts with ONE `level.dat` read and ONE `read_dir` per
/// world — not one per (pack, world) pair.
///
/// A world whose `level.dat` cannot be read does NOT fail the listing: reading
/// N worlds instead of one multiplies the chance of hitting a locked file
/// (`WorldInUse` is what a running Minecraft produces), and one locked world
/// must not blank the whole screen. Its packs report `state: None` instead.
async fn gather(instance_root: &Path) -> Vec<WorldFacts> {
    let saves_dir = instance_root.join(".minecraft").join("saves");
    let Ok(rd) = std::fs::read_dir(&saves_dir) else {
        return Vec::new();
    };
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
        let on_disk = world_link::list_on_disk_entries(&world_dir.join("datapacks")).await;
        let lists = world_link::read_level_dat_or_empty(&world_dir)
            .ok()
            .map(|(root, _framing)| level_dat::lists(&root));
        out.push(WorldFacts {
            world,
            on_disk,
            lists,
        });
    }
    out.sort_by(|a, b| a.world.to_lowercase().cmp(&b.world.to_lowercase()));
    out
}

/// The state of one named pack in one world.
///
/// `file_present` comes from the directory listing, not from level.dat: it is
/// [`state::derive`]'s first argument and is not recoverable from the two
/// lists — every Enabled/Disabled vs Orphaned/NotAdded distinction turns on it.
/// Membership is tested case-insensitively because NTFS is: a level.dat entry
/// spelled `file/VeinMiner.zip` and an on-disk `veinminer.zip` are one file,
/// and an exact test reports a disabled pack as enabled.
fn state_in(facts: &WorldFacts, filename: &str) -> Option<crate::datapacks::WorldPackState> {
    let (enabled, disabled) = facts.lists.as_ref()?;
    let file_present = facts
        .on_disk
        .iter()
        .any(|n| n.eq_ignore_ascii_case(filename));
    let entry = level_dat_entry(filename);
    Some(state::derive(
        file_present,
        world_link::contains_ci(enabled, &entry),
        world_link::contains_ci(disabled, &entry),
    ))
}

/// Every datapack this instance knows about, with its state in every world.
///
/// `expected` is the instance's expected `pack_format`, resolved by the caller
/// (the command layer owns the `AppHandle` needed to find the client jar).
pub async fn list_at(instance_root: &Path, expected: Option<u32>) -> Result<DatapackLibraryView> {
    let rows: Vec<InstalledDatapack> = registry::list(instance_root).await?;
    let worlds = gather(instance_root).await;

    // The entry set is the registry UNION every name present in some world.
    // A pack removed from the library without cascading is gone from the
    // registry but still loading in game; leaving it out would make live
    // content invisible. Keyed case-insensitively, preferring the registry's
    // spelling when both exist, because that is the name every write path uses.
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    for facts in &worlds {
        for n in &facts.on_disk {
            names.insert(n.to_lowercase(), n.clone());
        }
    }
    for r in &rows {
        names.insert(r.filename.to_lowercase(), r.filename.clone());
    }

    let entries = names
        .into_values()
        .map(|filename| {
            let row = rows
                .iter()
                .find(|r| r.filename.eq_ignore_ascii_case(&filename));
            let placements = worlds
                .iter()
                .filter(|f| {
                    // Only worlds that actually reference the pack — a world
                    // that has never seen it contributes nothing, and listing
                    // every world against every pack would turn the expander
                    // into noise.
                    f.on_disk.iter().any(|n| n.eq_ignore_ascii_case(&filename))
                        || f.lists.as_ref().is_some_and(|(en, dis)| {
                            let e = level_dat_entry(&filename);
                            world_link::contains_ci(en, &e) || world_link::contains_ci(dis, &e)
                        })
                })
                .map(|f| DatapackPlacementView {
                    world: f.world.clone(),
                    state: state_in(f, &filename),
                })
                .collect();

            DatapackLibraryEntry {
                in_library: row.is_some(),
                compat: compat_of(row.and_then(|r| r.pack_format), expected),
                pack: row.cloned().unwrap_or_else(|| unlisted(&filename)),
                placements,
            }
        })
        .collect();

    Ok(DatapackLibraryView {
        expected_pack_format: expected,
        entries,
    })
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
        pack_format: None,
        name: filename.trim_end_matches(".zip").to_string(),
        source: None,
        project_id: None,
        version_id: None,
        version_number: None,
        installed_at: String::new(),
    }
}

/// Same rule as `world_link`'s own compat check: `Unknown` unless both the
/// pack's format and the instance's expectation are known.
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
    use crate::datapacks::{library, world_link, WorldPackState};
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn datapack_zip(pack_format: u32) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(format!(r#"{{"pack":{{"pack_format":{pack_format}}}}}"#).as_bytes())
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
        std::fs::create_dir_all(
            root.join(".minecraft")
                .join("saves")
                .join(world)
                .join("datapacks"),
        )
        .unwrap();
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

        let view = list_at(td.path(), Some(48)).await.unwrap();
        let e = entry_for(&view, "vm.zip");

        assert!(e.in_library);
        assert_eq!(e.compat, PackCompat::Compatible);
        assert_eq!(
            e.placements,
            vec![
                DatapackPlacementView {
                    world: "Alpha".into(),
                    state: Some(WorldPackState::Enabled)
                },
                DatapackPlacementView {
                    world: "Beta".into(),
                    state: Some(WorldPackState::Disabled)
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
                state: Some(WorldPackState::Enabled)
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
        // Corrupt Broken's level.dat: present but unparseable, which is NOT the
        // same as absent (absent reads as two empty lists, a real answer).
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
    }

    #[tokio::test]
    async fn a_mismatched_pack_format_is_reported_once_on_the_row() {
        let td = tempfile::tempdir().unwrap();
        seed(td.path(), "vm.zip", 48).await;

        let view = list_at(td.path(), Some(57)).await.unwrap();

        assert_eq!(
            entry_for(&view, "vm.zip").compat,
            PackCompat::Mismatch {
                pack_format: 48,
                expected: 57
            }
        );
        assert_eq!(view.expected_pack_format, Some(57));
    }
}

//! Which worlds hold a given library pack, with an identity verdict per
//! world, plus the same-name reinstall fan-out built on top of it. See the
//! parent module doc for the lock rules.

use std::path::{Path, PathBuf};

use crate::datapacks::library_dir_at;
use crate::mods::store::{materialize, LinkPolicy};

use super::level_dat_lock;

/// One world holding an entry by a given name, and whether that entry is
/// provably the library's own content.
pub(crate) struct WorldPlacement {
    pub world: String,
    pub path: PathBuf,
    /// The world-side entry's sha1 equals the library entry's. `false` means a
    /// same-named entry the user (or a world import) put there — replacing it
    /// would be data loss, so every MUTATING caller must skip it and say so.
    pub is_ours: bool,
}

/// Every world whose `datapacks/` folder holds an entry named `filename`, with
/// world NAMES rather than paths, and an identity verdict per world.
///
/// This replaced a `worlds_linking` helper that answered only "does an entry
/// with this name exist there". That was enough for a same-name reinstall
/// (same name, same intent, and `materialize` writes the same bytes either
/// way), but NOT for an update, which deletes the old entry and links a
/// differently-named one: a same-named entry that is not ours must be left
/// alone. `materialize` commits by unconditional rename over the destination,
/// so a name-only check lets the update path silently destroy a pack the user
/// installed by hand — the F5 defect, reintroduced on the update path. The old
/// helper was deleted rather than kept, so no future caller can reach for the
/// unsafe one by mistake.
///
/// A directory entry is never `is_ours`: a folder datapack has no file sha1 to
/// compare against the library's zip.
pub(crate) async fn placements_of(instance_root: &Path, filename: &str) -> Vec<WorldPlacement> {
    let lib_sha = match tokio::fs::read(library_dir_at(instance_root).join(filename)).await {
        Ok(bytes) => Some(crate::datapacks::library::sha1_hex(&bytes)),
        Err(_) => None,
    };
    placements_against(instance_root, filename, lib_sha.as_deref()).await
}

/// [`placements_of`]'s body, with the identity reference supplied by the
/// caller instead of read from the library file. The distinction matters on
/// the reinstall path: by the time the fan-out runs, the library file already
/// holds the NEW bytes, so hashing against it would classify every
/// legitimately-linked-but-stale world as foreign. The caller captures the
/// OLD file's sha1 before replacing it and passes it here.
async fn placements_against(
    instance_root: &Path,
    filename: &str,
    lib_sha: Option<&str>,
) -> Vec<WorldPlacement> {
    let saves_dir = instance_root.join(".minecraft").join("saves");
    let Ok(rd) = std::fs::read_dir(&saves_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let Some(world) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        // Match `worlds::list_worlds`' own filter: without it a dot-directory
        // under `saves/` would be treated as a world, and `world_dirs_checked`
        // would then reject it downstream with a confusing path error instead
        // of it never having been offered.
        if crate::worlds::fs::validate_segment(&world).is_err() {
            continue;
        }
        let candidate = entry.path().join("datapacks").join(filename);
        let Ok(cand_meta) = tokio::fs::metadata(&candidate).await else {
            continue;
        };
        let is_ours = if cand_meta.is_dir() {
            false
        } else {
            match (lib_sha, tokio::fs::read(&candidate).await) {
                (Some(lib), Ok(bytes)) => crate::datapacks::library::sha1_hex(&bytes) == lib,
                _ => false,
            }
        };
        out.push(WorldPlacement {
            world,
            path: candidate,
            is_ours,
        });
    }
    out
}

/// The same-name reinstall fan-out: push the library's (already replaced)
/// bytes into every world whose `datapacks/` holds this filename AND whose
/// current content is provably the pack being replaced — `expected_sha` is the
/// OLD library file's hash, captured by the caller before `place_bytes` ran.
/// A same-named entry that does not match is a pack the user (or a world
/// import) put there themselves; replacing it would be the F5 data loss, so it
/// is skipped and reported. `None` (fresh install, or the old file was
/// unreadable) skips every candidate — the safe direction.
///
/// level.dat is deliberately never touched: each world's own enabled/disabled
/// choice stands, which is why the per-world outcome is
/// [`WorldMigration::Refreshed`], not `Migrated`.
///
/// Takes [`level_dat_lock`] once, and snapshots the placements INSIDE it, so
/// a concurrent locked removal cannot interleave between the snapshot and the
/// writes — without the lock, `remove_from_world_at` could delete a world's
/// file and level.dat entry and this fan-out would then re-materialize the
/// file from its stale snapshot, leaving it present-and-unlisted, which
/// Minecraft auto-enables: a silently resurrected, just-removed pack.
pub(crate) async fn refresh_placements(
    instance_root: &Path,
    filename: &str,
    expected_sha: Option<&str>,
) -> Vec<crate::datapacks::WorldMigration> {
    use crate::datapacks::WorldMigration;

    let src = library_dir_at(instance_root).join(filename);

    let _guard = level_dat_lock().lock().await;

    let placements = placements_against(instance_root, filename, expected_sha).await;
    let mut report = Vec::with_capacity(placements.len());
    for p in placements {
        if !p.is_ours {
            report.push(WorldMigration::SkippedNotOurs { world: p.world });
            continue;
        }
        match materialize(&src, &p.path, LinkPolicy::LinkIfPossible).await {
            Ok(_) => report.push(WorldMigration::Refreshed { world: p.world }),
            Err(e) => report.push(WorldMigration::Failed {
                world: p.world,
                details: e.details(),
            }),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::world_link::add_to_world_at;
    use crate::datapacks::world_link::test_util::*;

    #[tokio::test]
    async fn placements_of_marks_only_the_library_content_as_ours() {
        let _lock = hardlink_lock();
        let td = tempfile::tempdir().unwrap();
        seed_library(td.path(), "vm.zip", 48).await;
        let saves = td.path().join(".minecraft").join("saves");
        std::fs::create_dir_all(saves.join("Ours")).unwrap();
        std::fs::create_dir_all(saves.join("Theirs").join("datapacks")).unwrap();
        add_to_world_at(td.path(), "Ours", "vm.zip").await.unwrap();
        // Same NAME, different content — a pack the user dropped in by hand.
        std::fs::write(
            saves.join("Theirs").join("datapacks").join("vm.zip"),
            datapack_zip(57),
        )
        .unwrap();

        let found = placements_of(td.path(), "vm.zip").await;

        let mut ours: Vec<&str> = found
            .iter()
            .filter(|p| p.is_ours)
            .map(|p| p.world.as_str())
            .collect();
        ours.sort_unstable();
        assert_eq!(ours, vec!["Ours"]);
        let theirs = found
            .iter()
            .find(|p| p.world == "Theirs")
            .expect("Theirs must still be listed, just not claimed");
        assert!(!theirs.is_ours);
    }
}

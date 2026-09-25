//! Which worlds hold a given library pack, with an identity verdict per
//! world, plus the same-name reinstall fan-out built on top of it. See the
//! parent module doc for the lock rules.

use std::path::{Path, PathBuf};

use crate::datapacks::detect::Resolved;
use crate::datapacks::library_dir_at;
use crate::error::{Error, Result};
use crate::mods::store::{materialize, LinkPolicy};

use super::level_dat_lock;

/// One world holding an entry by a given name, and whether that entry is
/// provably the library's own content.
pub(crate) struct WorldPlacement {
    pub world: String,
    pub path: PathBuf,
    /// The on-disk spelling (R2). The engine's id is `file/` + this, not the
    /// library's spelling.
    pub entry_name: String,
    /// The world-side entry's sha1 equals the library entry's. `false` means a
    /// same-named entry the user (or a world import) put there — replacing it
    /// would be data loss, so every MUTATING caller must skip it and say so.
    pub is_ours: bool,
}

/// What a scan of every world found for one name.
pub(crate) struct Placements {
    /// Worlds holding an entry by the name, each with its identity verdict.
    pub found: Vec<WorldPlacement>,
    /// Worlds Lucerna could not check for the name, each with the reason: its
    /// `datapacks/` could not be listed, R2 could not tell (a stat error), or
    /// the entry could not be read to compare. "Could not check" is not "does
    /// not hold it" (Fallback discipline Q1): every caller reports these as a
    /// failure, which keeps the library copy so a retry converges.
    pub unchecked: Vec<(String, String)>,
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
///
/// `Err` = the library copy, or `saves/` itself, could not be read: no world
/// can be verified then, and there is no world to name in a report. A missing
/// library copy or `saves/` is a fact, not an error.
pub(crate) async fn placements_of(instance_root: &Path, filename: &str) -> Result<Placements> {
    let lib = library_dir_at(instance_root).join(filename);
    let lib_sha = match tokio::fs::read(&lib).await {
        Ok(bytes) => Some(crate::datapacks::library::sha1_hex(&bytes)),
        // No library copy (a pack left only in worlds): nothing can be ours.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(Error::io(lib.display().to_string(), e)),
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
) -> Result<Placements> {
    let saves_dir = instance_root.join(".minecraft").join("saves");
    let rd = match std::fs::read_dir(&saves_dir) {
        Ok(rd) => rd,
        // No saves/ at all: no world can hold the pack.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Placements {
                found: Vec::new(),
                unchecked: Vec::new(),
            })
        }
        Err(e) => return Err(Error::io(saves_dir.display().to_string(), e)),
    };
    let mut out = Placements {
        found: Vec::new(),
        unchecked: Vec::new(),
    };
    for entry in rd {
        // An entry that cannot be read names no world to report.
        let entry = entry.map_err(|e| Error::io(saves_dir.display().to_string(), e))?;
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
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(e) => {
                out.unchecked
                    .push((world, format!("could not read its folder: {e}")));
                continue;
            }
        };
        if !meta.is_dir() {
            continue;
        }
        let dp = entry.path().join("datapacks");
        // R2 (N.3): the entry this library name denotes in this world.
        let entry_name = match super::resolve_on_disk(&dp, filename).await {
            Ok((_, Resolved::Exact(n) | Resolved::Folded(n))) => n,
            Ok((_, Resolved::Absent)) => continue,
            Ok((_, Resolved::Unknown(e))) => {
                out.unchecked.push((
                    world,
                    format!(
                        "could not tell whether {} holds {filename}: {e}",
                        dp.display()
                    ),
                ));
                continue;
            }
            Err(e) => {
                out.unchecked
                    .push((world, format!("could not list {}: {e}", dp.display())));
                continue;
            }
        };
        let candidate = dp.join(&entry_name);
        let cand_meta = match tokio::fs::metadata(&candidate).await {
            Ok(meta) => meta,
            // Gone since the listing: the world no longer holds it.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                out.unchecked.push((
                    world,
                    format!("could not read {}: {e}", candidate.display()),
                ));
                continue;
            }
        };
        let is_ours = match (cand_meta.is_dir(), lib_sha) {
            // A folder is never the library's zip, and with no library copy
            // nothing can be proven ours.
            (true, _) | (false, None) => false,
            (false, Some(lib)) => match tokio::fs::read(&candidate).await {
                Ok(bytes) => crate::datapacks::library::sha1_hex(&bytes) == lib,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                // Could not compare: not "not ours", which a caller would
                // leave alone and then report the job done.
                Err(e) => {
                    out.unchecked.push((
                        world,
                        format!("could not read {}: {e}", candidate.display()),
                    ));
                    continue;
                }
            },
        };
        out.found.push(WorldPlacement {
            world,
            path: candidate,
            entry_name,
            is_ours,
        });
    }
    Ok(out)
}

/// The same-name reinstall fan-out: push the library's (already replaced)
/// bytes into every world whose `datapacks/` holds this filename AND whose
/// current content is provably the pack being replaced — `expected_sha` is the
/// OLD library file's hash, captured by the caller before `place_bytes` ran.
/// A same-named entry that does not match is a pack the user (or a world
/// import) put there themselves; replacing it would be the F5 data loss, so it
/// is skipped and reported. `None` (fresh install, or the old file was
/// unreadable) skips every candidate — the safe direction. A world that could
/// not be checked is reported `Failed`: it may still hold the stale bytes.
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
///
/// A FRESH install (`expected_sha: None`) has no bytes of ours in any world,
/// so a world it could not check cannot hold a stale copy: it reports
/// nothing for it. A reinstall that cannot even list `saves/` reports one
/// `Failed` entry naming that folder: the install itself went through, and
/// the worlds may still be on the old bytes.
pub(crate) async fn refresh_placements(
    instance_root: &Path,
    filename: &str,
    expected_sha: Option<&str>,
) -> Vec<crate::datapacks::WorldMigration> {
    use crate::datapacks::WorldMigration;

    let src = library_dir_at(instance_root).join(filename);

    let _guard = level_dat_lock().lock().await;

    let placements = match placements_against(instance_root, filename, expected_sha).await {
        Ok(placements) => placements,
        Err(e) if expected_sha.is_none() => {
            crate::diag!(
                "datapacks: fresh install of {filename} could not list the worlds, which is \
                 harmless (none can hold it yet): {e}"
            );
            return Vec::new();
        }
        Err(e) => {
            let saves = instance_root.join(".minecraft").join("saves");
            return vec![WorldMigration::Failed {
                world: saves.display().to_string(),
                details: format!("no world was refreshed: {e}"),
            }];
        }
    };
    let mut report = Vec::with_capacity(placements.found.len() + placements.unchecked.len());
    for p in placements.found {
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
    // Only a reinstall can have left stale bytes of ours in a world.
    if expected_sha.is_some() {
        for (world, details) in placements.unchecked {
            report.push(WorldMigration::Failed { world, details });
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
        game_world(td.path(), "Ours");
        std::fs::create_dir_all(saves.join("Theirs").join("datapacks")).unwrap();
        add_to_world_at(td.path(), "Ours", "vm.zip").await.unwrap();
        // Same NAME, different content — a pack the user dropped in by hand.
        std::fs::write(
            saves.join("Theirs").join("datapacks").join("vm.zip"),
            datapack_zip(57),
        )
        .unwrap();

        let found = placements_of(td.path(), "vm.zip").await.unwrap().found;

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

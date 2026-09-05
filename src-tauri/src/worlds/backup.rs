//! Backup-side operations.

use crate::error::{Error, Result};
use crate::worlds::{backups_root, fs as wfs, world_dir, zip as wzip, Backup};
use chrono::Utc;
use std::path::{Path, PathBuf};

/// Create a new backup of `world_folder_name` under
/// `<instance>/backups/<world>/`. Filename is the current UTC
/// timestamp formatted as `YYYY-MM-DDTHH-mm-ss.zip`. On sub-second
/// collision (rapid clicks), suffix `.2.zip`, `.3.zip`, …, up to
/// `.99.zip` before erroring `WorldNameUnresolvable`.
pub async fn backup_world(
    app: &tauri::AppHandle,
    instance_id: &str,
    world_folder_name: &str,
) -> Result<Backup> {
    let world_path = world_dir(app, instance_id, world_folder_name)?;
    let backups_dir = backups_root(app, instance_id)?.join(world_folder_name);
    std::fs::create_dir_all(&backups_dir)
        .map_err(|e| Error::io(backups_dir.display().to_string(), e))?;

    let base = Utc::now().format("%Y-%m-%dT%H-%M-%S").to_string();
    let (filename, dest_zip) = pick_unused_filename(&backups_dir, &base)?;

    // Offload the CPU-heavy zip op so the IPC thread stays responsive.
    let world_path_owned = world_path.clone();
    let dest_zip_owned = dest_zip.clone();
    let world_folder_name_owned = world_folder_name.to_string();
    tokio::task::spawn_blocking(move || {
        wzip::zip_dir(&world_path_owned, &dest_zip_owned, &world_folder_name_owned)
    })
    .await
    .map_err(|e| Error::io(dest_zip.display().to_string(), format!("join: {e}")))??;

    let size_bytes = std::fs::metadata(&dest_zip)
        .map(|m| m.len())
        .map_err(|e| Error::io(dest_zip.display().to_string(), e))? as f64;
    let created_unix_ms = parse_timestamp_from_filename(&filename).unwrap_or(0.0);
    Ok(Backup {
        filename,
        size_bytes,
        created_unix_ms,
    })
}

/// Try `<base>.zip`, then `<base>.2.zip`, …, up to `<base>.99.zip`.
/// Returns the chosen (filename, full_path) or
/// `WorldNameUnresolvable` if all 99 are taken.
pub(crate) fn pick_unused_filename(
    backups_dir: &std::path::Path,
    base: &str,
) -> Result<(String, PathBuf)> {
    for i in 1..=99 {
        let filename = if i == 1 {
            format!("{base}.zip")
        } else {
            format!("{base}.{i}.zip")
        };
        let p = backups_dir.join(&filename);
        // `try_exists`, not `exists`: `exists()` answers false for ANY stat
        // failure, so a transient error would report an occupied name as free —
        // and `zip_dir` opens its destination with `File::create`, which
        // truncates. That is a user's backup gone. Unread ⇒ occupied is the
        // restrictive direction (CLAUDE.md, Fallback discipline, question 1).
        if !p.try_exists().unwrap_or(true) {
            return Ok((filename, p));
        }
    }
    Err(Error::WorldNameUnresolvable {
        folder_name: base.into(),
    })
}

/// What `move_set_at` did with one world's backup set. `moved` zips are under
/// the destination; `left` zips could not be renamed or given a free name and
/// are still under the source directory, untouched; `ignored` entries are not
/// backups at all (a stray file, a directory, a non-UTF-8 name) and were never
/// touched. The split matters for the user-facing sentence: "{n} backups stayed
/// in the source" must count backups only — a README beside the zips is not a
/// backup that stayed behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoveReport {
    pub moved: u32,
    pub left: u32,
    pub ignored: u32,
}

/// The stem of a backup zip name — `Some("2026-01-01T00-00-00")` for
/// `2026-01-01T00-00-00.zip` — or `None` for anything that is not one.
///
/// ASCII-case-insensitive on the extension (`.ZIP` is a backup the user renamed
/// by hand), a wider net than `list_backups`' exact `zip`. `pick_unused_filename`
/// rebuilds the name as `<stem>.zip`, so a moved `.ZIP` lands under the spelling
/// every listing in this module looks for. A bare `.zip` has no stem and is not
/// a backup. The cut is checked to be a char boundary so a name ending in a
/// multi-byte character can never panic `split_at`.
fn zip_stem(name: &str) -> Option<&str> {
    let cut = name.len().checked_sub(4)?;
    if !name.is_char_boundary(cut) {
        return None;
    }
    let (stem, ext) = name.split_at(cut);
    (ext.eq_ignore_ascii_case(".zip") && !stem.is_empty()).then_some(stem)
}

/// Move one world's backup set — every `*.zip` directly under `src_dir`
/// (`<src instance>/backups/<world>/`) — into `dst_dir`
/// (`<dst instance>/backups/<final>/`), one `fs::rename` per file. Used by a
/// world **move** (spec §4.2 step 8, A8); a copy leaves backups with the
/// original (D4).
///
/// **Per file, not a directory rename.** `<dst>/backups/` is created lazily by
/// the first backup, so a directory rename fails on a target that never backed
/// anything up; and `dst_dir` may already hold an orphaned set of the same
/// name, which a directory rename would refuse (or, on Linux, replace if
/// empty). Per-file renames through `pick_unused_filename` merge into such a
/// set and never overwrite: an incoming `<stem>.zip` whose name is taken lands
/// as `<stem>.2.zip`. Merging is the right answer because names are the only
/// key backups have — `count_backups` and `list_backups` look up
/// `backups/<folder name>/`, so a world imported today under a name whose
/// backup set outlived its predecessor inherits that set; a migrated world
/// arriving next to an orphaned set of its name gets exactly the same
/// treatment.
///
/// **Rename-only — deliberately no copy fallback.** A zip that cannot be renamed
/// (or given a free name) stays in `src_dir`, is counted in `left`, and is
/// `diag!`-logged. That is a safe place: while the source world still exists it
/// is listed as that world's backup, and once the world is gone it is what
/// `orphaned_backup_sets_at` lists under "backups without a world", where the
/// user can already see and restore it. A copy would need a write primitive
/// this tree is not allowed (`structural_no_inplace_mods_write`), double the
/// disk footprint of a set that may be gigabytes, and leave a source copy to
/// delete — a second failure mode for a file that is already safe.
///
/// **Fallback discipline.** `read_dir(src_dir)` failing with `NotFound` is the
/// ordinary "never backed up" case and answers zero without touching
/// `dst_dir`; any other listing failure is `Err` — answering zero to a
/// permission failure would let a Move report success while the whole set
/// stays behind unmentioned. The listing is drained before the first rename,
/// so an `Err` from any point before the renames means nothing has moved.
/// `dst_dir` is only created when there is a zip to put in it. Entries that
/// are not `.zip` files are never touched and count in `ignored`. `src_dir` is
/// removed only when nothing was left and a fresh listing shows it empty;
/// `NotFound` there means it is already gone, and any other removal failure is
/// `Err` — logged with the moved count first, because by then the zips HAVE
/// moved and Logs must say so.
pub fn move_set_at(src_dir: &Path, dst_dir: &Path) -> Result<MoveReport> {
    let entries = match std::fs::read_dir(src_dir) {
        Ok(rd) => rd,
        // `backups/<world>/` is created lazily by the first backup: absent means
        // "this world was never backed up", and that is the only failure that
        // means "nothing here".
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MoveReport::default());
        }
        // "Could not tell" resolves to an error, not to zero — the restrictive
        // direction (CLAUDE.md, Fallback discipline, question 1).
        Err(e) => return Err(Error::io(src_dir.display().to_string(), e)),
    };

    let mut report = MoveReport::default();
    // (name as on disk, stem) — drained before the first rename so an entry
    // that cannot be read fails the call while the set is still whole.
    let mut zips: Vec<(String, String)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(src_dir.display().to_string(), e))?;
        let file_name = entry.file_name();
        match file_name.to_str().and_then(zip_stem) {
            Some(stem) => {
                zips.push((file_name.to_string_lossy().into_owned(), stem.to_string()));
            }
            // A stray file, a directory, a non-UTF-8 name: not a backup, so it
            // is never touched — counted apart from `left`, so the caller can
            // say the directory was not moved whole without calling a README a
            // backup that stayed behind.
            None => report.ignored = report.ignored.saturating_add(1),
        }
    }

    if !zips.is_empty() {
        std::fs::create_dir_all(dst_dir)
            .map_err(|e| Error::io(dst_dir.display().to_string(), e))?;
    }
    for (name, stem) in &zips {
        let from = src_dir.join(name);
        let to = match pick_unused_filename(dst_dir, stem) {
            Ok((_, to)) => to,
            Err(e) => {
                crate::diag!(
                    "backup move: no free name for {} under {}: {e}; left in place",
                    from.display(),
                    dst_dir.display()
                );
                report.left = report.left.saturating_add(1);
                continue;
            }
        };
        // `pick_unused_filename` probed `to` with `try_exists` (unread ⇒ taken).
        // `fs::rename` itself replaces an existing FILE on every platform, so
        // that probe is what keeps an orphaned set's zips intact. The
        // probe-to-rename gap is microseconds, and the only launcher writer
        // into `<dst>/backups/` — `backup_world` — is refused on the target for
        // the whole migration by the maintenance claim (spec §4.0). Stated,
        // not closed, the way `import.rs` states its own free-name gap.
        match std::fs::rename(&from, &to) {
            Ok(()) => report.moved = report.moved.saturating_add(1),
            // Checked, counted, logged, and the loop goes on: one zip that
            // will not move must not strand the rest of the set.
            Err(e) => {
                crate::diag!(
                    "backup move: {} -> {} failed: {e}; left in place",
                    from.display(),
                    to.display()
                );
                report.left = report.left.saturating_add(1);
            }
        }
    }

    if report.left == 0 && report.ignored == 0 {
        if let Err(e) = remove_emptied_dir(src_dir) {
            crate::diag!(
                "backup move: all {} zips moved to {}, but the emptied {} could not be removed: {e}",
                report.moved,
                dst_dir.display(),
                src_dir.display()
            );
            return Err(e);
        }
    }
    Ok(report)
}

/// Remove `dir` when a fresh listing shows it empty.
///
/// A listing that cannot be completed — including a first entry that fails to
/// read — counts as "not empty": the direction that never removes a directory
/// with something still in it. `NotFound` on either step means the directory
/// is already gone, which is the state being asked for. Something that
/// appeared since the caller's own listing (Explorer can drop a file here;
/// nothing in the launcher does meanwhile) leaves the directory in place, and
/// that is not a failure.
fn remove_emptied_dir(dir: &Path) -> Result<()> {
    let is_empty = match std::fs::read_dir(dir) {
        Ok(mut rd) => {
            let empty = rd.next().is_none();
            // Release the enumeration handle before the removal: Windows
            // refuses to remove a directory that is still being listed.
            drop(rd);
            empty
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::io(dir.display().to_string(), e)),
    };
    if !is_empty {
        return Ok(());
    }
    match std::fs::remove_dir(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(dir.display().to_string(), e)),
    }
}

/// Parse the timestamp portion of a backup filename into ms-since-epoch.
/// Accepts both `<ISO>.zip` and `<ISO>.<n>.zip`. Returns None on shape
/// mismatch (e.g. a pre-restore zip with `pre-restore-<ISO>.zip`,
/// or any other naming scheme — caller handles None by surfacing 0).
pub fn parse_timestamp_from_filename(name: &str) -> Option<f64> {
    let stem = name.strip_suffix(".zip")?;
    // Drop a trailing `.<n>` collision suffix if present.
    let stem = match stem.rsplit_once('.') {
        Some((head, tail)) if tail.chars().all(|c| c.is_ascii_digit()) => head,
        _ => stem,
    };
    // Drop a `pre-restore-` prefix if present.
    let iso = stem.strip_prefix("pre-restore-").unwrap_or(stem);
    // YYYY-MM-DDTHH-MM-SS — last two `-` are time separators.
    // chrono needs `:` for the time portion. Surgery:
    if iso.len() < 19 {
        return None;
    }
    let (date, time_dashes) = iso.split_at(10); // "YYYY-MM-DD"
    let time = time_dashes.trim_start_matches('T').replacen('-', ":", 2);
    let combined = format!("{date}T{time}");
    let dt = chrono::NaiveDateTime::parse_from_str(&combined, "%Y-%m-%dT%H:%M:%S").ok()?;
    Some(dt.and_utc().timestamp_millis() as f64)
}

/// List every `.zip` under `<instance>/backups/<world>/`, sorted
/// newest-first by parsed timestamp (filename-encoded). Missing dir
/// → empty Vec.
pub fn list_backups(
    app: &tauri::AppHandle,
    instance_id: &str,
    world_folder_name: &str,
) -> Result<Vec<Backup>> {
    wfs::validate_segment(world_folder_name)?;
    let world_backups = backups_root(app, instance_id)?.join(world_folder_name);
    if !world_backups.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&world_backups)
        .map_err(|e| Error::io(world_backups.display().to_string(), e))?
    {
        let entry = entry.map_err(|e| Error::io(world_backups.display().to_string(), e))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("zip") {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|s| s.to_str()).map(String::from) else {
            continue;
        };
        let meta = entry
            .metadata()
            .map_err(|e| Error::io(path.display().to_string(), e))?;
        let size_bytes = meta.len() as f64;
        // Prefer the filename-encoded timestamp; fall back to mtime if
        // parsing fails (handles a file the user dropped in manually).
        let created_unix_ms = parse_timestamp_from_filename(&filename).unwrap_or_else(|| {
            meta.modified()
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                })
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0)
        });
        out.push(Backup {
            filename,
            size_bytes,
            created_unix_ms,
        });
    }
    out.sort_by(|a, b| {
        b.created_unix_ms
            .partial_cmp(&a.created_unix_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

/// Delete a single backup zip.
/// Returns BackupNotFound if the file doesn't exist (caller can ignore or surface).
pub fn delete_backup(
    app: &tauri::AppHandle,
    instance_id: &str,
    world_folder_name: &str,
    backup_filename: &str,
) -> Result<()> {
    wfs::validate_segment(world_folder_name)?;
    wfs::validate_segment(backup_filename)?;
    let p = backups_root(app, instance_id)?
        .join(world_folder_name)
        .join(backup_filename);
    if !p.exists() {
        return Err(Error::BackupNotFound {
            instance_id: instance_id.into(),
            world_folder: world_folder_name.into(),
            filename: backup_filename.into(),
        });
    }
    std::fs::remove_file(&p).map_err(|e| Error::io(p.display().to_string(), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn pick_unused_filename_first_slot_when_empty() {
        let td = tempdir().unwrap();
        let (name, path) = pick_unused_filename(td.path(), "2026-05-24T10-00-00").unwrap();
        assert_eq!(name, "2026-05-24T10-00-00.zip");
        assert_eq!(path, td.path().join("2026-05-24T10-00-00.zip"));
    }

    #[test]
    fn pick_unused_filename_suffixes_on_collision() {
        let td = tempdir().unwrap();
        fs::write(td.path().join("2026-05-24T10-00-00.zip"), b"x").unwrap();
        let (name, _p) = pick_unused_filename(td.path(), "2026-05-24T10-00-00").unwrap();
        assert_eq!(name, "2026-05-24T10-00-00.2.zip");
    }

    #[test]
    fn pick_unused_filename_errors_after_99_collisions() {
        let td = tempdir().unwrap();
        fs::write(td.path().join("b.zip"), b"x").unwrap();
        for i in 2..=99 {
            fs::write(td.path().join(format!("b.{i}.zip")), b"x").unwrap();
        }
        assert!(matches!(
            pick_unused_filename(td.path(), "b"),
            Err(Error::WorldNameUnresolvable { .. })
        ));
    }

    #[test]
    fn parse_timestamp_simple_iso() {
        let ms = parse_timestamp_from_filename("2026-05-24T15-30-12.zip").unwrap();
        // 2026-05-24T15:30:12 UTC → 1779672612000 ms (sanity-check just
        // the bottom three digits are zeros for a whole-second value).
        assert_eq!(ms as u64 % 1000, 0);
        assert!(ms > 0.0);
    }

    #[test]
    fn parse_timestamp_with_collision_suffix() {
        let a = parse_timestamp_from_filename("2026-05-24T15-30-12.zip").unwrap();
        let b = parse_timestamp_from_filename("2026-05-24T15-30-12.2.zip").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn parse_timestamp_pre_restore_prefix() {
        let a = parse_timestamp_from_filename("2026-05-24T15-30-12.zip").unwrap();
        let b = parse_timestamp_from_filename("pre-restore-2026-05-24T15-30-12.zip").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn parse_timestamp_returns_none_for_garbage() {
        assert!(parse_timestamp_from_filename("not-a-timestamp.zip").is_none());
        assert!(parse_timestamp_from_filename("2026-05-24.zip").is_none());
        assert!(parse_timestamp_from_filename("nodotzip").is_none());
    }

    use crate::worlds::Backup;

    #[test]
    fn list_backups_filename_parse_orders_newest_first() {
        // Local pure-fn test of the sort order — we exercise the
        // ordering logic without an AppHandle by constructing
        // Backup structs directly.
        let mut backups = vec![
            Backup {
                filename: "2026-05-20T10-00-00.zip".into(),
                size_bytes: 1.0,
                created_unix_ms: parse_timestamp_from_filename("2026-05-20T10-00-00.zip").unwrap(),
            },
            Backup {
                filename: "2026-05-24T10-00-00.zip".into(),
                size_bytes: 1.0,
                created_unix_ms: parse_timestamp_from_filename("2026-05-24T10-00-00.zip").unwrap(),
            },
        ];
        backups.sort_by(|a, b| {
            b.created_unix_ms
                .partial_cmp(&a.created_unix_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        assert_eq!(backups[0].filename, "2026-05-24T10-00-00.zip");
    }
}

#[cfg(test)]
mod move_set_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Create `dir` and write each `(name, bytes)` into it.
    fn set_with(dir: &Path, files: &[(&str, &[u8])]) {
        fs::create_dir_all(dir).unwrap();
        for (name, bytes) in files {
            fs::write(dir.join(name), bytes).unwrap();
        }
    }

    /// On-disk entry names under `dir`, sorted — the real spelling, so a
    /// case-insensitive filesystem cannot hide a `.ZIP` that stayed `.ZIP`.
    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn zip_stem_matches_case_insensitively_and_rejects_non_zips() {
        assert_eq!(
            zip_stem("2026-01-01T00-00-00.zip"),
            Some("2026-01-01T00-00-00")
        );
        assert_eq!(
            zip_stem("2026-01-01T00-00-00.ZIP"),
            Some("2026-01-01T00-00-00")
        );
        assert_eq!(zip_stem("мир.Zip"), Some("мир"));
        assert_eq!(zip_stem(".zip"), None, "no stem is not a backup");
        assert_eq!(zip_stem("notes.txt"), None);
        assert_eq!(zip_stem("a.zip.bak"), None);
        assert_eq!(zip_stem("zip"), None);
        assert_eq!(
            zip_stem("é.zp"),
            None,
            "a cut inside a multi-byte character must not panic"
        );
    }

    #[test]
    fn a_missing_source_set_is_zero_and_creates_nothing() {
        let td = tempdir().unwrap();
        let src = td.path().join("src").join("backups").join("W");
        let dst = td.path().join("dst").join("backups").join("W");

        let report = move_set_at(&src, &dst).unwrap();

        assert_eq!(report, MoveReport::default());
        assert!(
            !td.path().join("dst").try_exists().unwrap(),
            "a world that was never backed up must not grow an empty set in the target"
        );
    }

    #[test]
    fn moves_every_zip_and_removes_the_emptied_source_dir() {
        let td = tempdir().unwrap();
        let src = td.path().join("src").join("backups").join("W");
        let dst = td.path().join("dst").join("backups").join("W");
        set_with(
            &src,
            &[
                ("2026-01-01T00-00-00.zip", &b"one"[..]),
                ("2026-01-02T00-00-00.zip", &b"two"[..]),
                // A hand-renamed `.ZIP` is a backup too.
                ("2026-01-03T00-00-00.ZIP", &b"three"[..]),
            ],
        );

        let report = move_set_at(&src, &dst).unwrap();

        assert_eq!(
            report,
            MoveReport {
                moved: 3,
                left: 0,
                ignored: 0
            }
        );
        assert_eq!(
            names_in(&dst),
            vec![
                "2026-01-01T00-00-00.zip",
                "2026-01-02T00-00-00.zip",
                // Re-spelled by `pick_unused_filename`, so `list_backups` finds it.
                "2026-01-03T00-00-00.zip",
            ]
        );
        assert_eq!(
            fs::read(dst.join("2026-01-01T00-00-00.zip")).unwrap(),
            b"one"
        );
        assert_eq!(
            fs::read(dst.join("2026-01-02T00-00-00.zip")).unwrap(),
            b"two"
        );
        assert_eq!(
            fs::read(dst.join("2026-01-03T00-00-00.zip")).unwrap(),
            b"three"
        );
        assert!(
            !src.try_exists().unwrap(),
            "an emptied source set leaves no shell behind"
        );
    }

    #[test]
    fn merges_into_an_orphaned_set_of_the_same_name_without_overwriting() {
        let td = tempdir().unwrap();
        let src = td.path().join("src").join("backups").join("W");
        let dst = td.path().join("dst").join("backups").join("W");
        // The target already holds a set under this name — a world of the same
        // name deleted outside the launcher, say — with a zip whose name the
        // incoming set also uses.
        set_with(&dst, &[("2026-01-01T00-00-00.zip", &b"orphan"[..])]);
        set_with(&src, &[("2026-01-01T00-00-00.zip", &b"incoming"[..])]);

        let report = move_set_at(&src, &dst).unwrap();

        assert_eq!(
            report,
            MoveReport {
                moved: 1,
                left: 0,
                ignored: 0
            }
        );
        assert_eq!(
            fs::read(dst.join("2026-01-01T00-00-00.zip")).unwrap(),
            b"orphan",
            "the orphaned set's zip must never be overwritten"
        );
        assert_eq!(
            fs::read(dst.join("2026-01-01T00-00-00.2.zip")).unwrap(),
            b"incoming",
            "the incoming zip takes the next free suffix `pick_unused_filename` gives it"
        );
        assert!(!src.try_exists().unwrap());
    }

    #[test]
    fn leaves_non_zip_entries_where_they_are_and_keeps_the_source_dir() {
        let td = tempdir().unwrap();
        let src = td.path().join("src").join("backups").join("W");
        let dst = td.path().join("dst").join("backups").join("W");
        set_with(
            &src,
            &[
                ("2026-01-01T00-00-00.zip", &b"one"[..]),
                ("notes.txt", &b"not a backup"[..]),
            ],
        );

        let report = move_set_at(&src, &dst).unwrap();

        assert_eq!(
            report,
            MoveReport {
                moved: 1,
                left: 0,
                ignored: 1
            }
        );
        assert_eq!(names_in(&dst), vec!["2026-01-01T00-00-00.zip"]);
        assert_eq!(
            names_in(&src),
            vec!["notes.txt"],
            "the source dir stays, holding what was not moved"
        );
        assert_eq!(fs::read(src.join("notes.txt")).unwrap(), b"not a backup");
    }

    #[test]
    fn creates_a_destination_backups_root_that_does_not_exist_yet() {
        let td = tempdir().unwrap();
        let src = td.path().join("src").join("backups").join("W");
        // `<dst>/backups/` is created lazily by the first backup; a target that
        // never backed anything up has none, and the set must still arrive —
        // under the (possibly suffixed) final world name.
        let dst = td.path().join("dst").join("backups").join("W (2)");
        set_with(&src, &[("2026-01-01T00-00-00.zip", &b"one"[..])]);
        assert!(!td.path().join("dst").try_exists().unwrap());

        let report = move_set_at(&src, &dst).unwrap();

        assert_eq!(
            report,
            MoveReport {
                moved: 1,
                left: 0,
                ignored: 0
            }
        );
        assert!(dst.is_dir());
        assert_eq!(
            fs::read(dst.join("2026-01-01T00-00-00.zip")).unwrap(),
            b"one"
        );
    }

    #[test]
    fn a_zip_with_no_free_name_stays_and_is_counted_while_the_rest_still_move() {
        let td = tempdir().unwrap();
        let src = td.path().join("src").join("backups").join("W");
        let dst = td.path().join("dst").join("backups").join("W");
        // Every name `pick_unused_filename` would try for `b` is taken — the
        // one "cannot place" case that is deterministic on every platform
        // without a seam.
        fs::create_dir_all(&dst).unwrap();
        fs::write(dst.join("b.zip"), b"x").unwrap();
        for i in 2..=99 {
            fs::write(dst.join(format!("b.{i}.zip")), b"x").unwrap();
        }
        set_with(&src, &[("b.zip", &b"stuck"[..]), ("c.zip", &b"free"[..])]);

        let report = move_set_at(&src, &dst).unwrap();

        assert_eq!(
            report,
            MoveReport {
                moved: 1,
                left: 1,
                ignored: 0
            }
        );
        assert_eq!(
            fs::read(src.join("b.zip")).unwrap(),
            b"stuck",
            "the zip that could not be placed is still in the source, untouched"
        );
        assert_eq!(
            fs::read(dst.join("b.zip")).unwrap(),
            b"x",
            "and nothing in the target was overwritten"
        );
        assert_eq!(
            fs::read(dst.join("c.zip")).unwrap(),
            b"free",
            "the loop carried on past the failure"
        );
        assert!(src.is_dir(), "a source dir that still holds a zip is kept");
    }

    #[test]
    fn an_empty_source_set_is_removed_and_no_target_dir_is_created() {
        let td = tempdir().unwrap();
        let src = td.path().join("src").join("backups").join("W");
        let dst = td.path().join("dst").join("backups").join("W");
        fs::create_dir_all(&src).unwrap();

        let report = move_set_at(&src, &dst).unwrap();

        assert_eq!(report, MoveReport::default());
        assert!(
            !src.try_exists().unwrap(),
            "an empty shell has nothing to offer the orphan UI"
        );
        assert!(!td.path().join("dst").try_exists().unwrap());
    }
}

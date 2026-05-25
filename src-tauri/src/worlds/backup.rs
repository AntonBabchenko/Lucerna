//! Backup-side operations.

use crate::error::{Error, Result};
use crate::worlds::{backups_root, fs as wfs, saves_dir, zip as wzip, Backup};
use chrono::Utc;
use std::path::PathBuf;

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
    wfs::validate_segment(world_folder_name)?;
    let saves = saves_dir(app, instance_id)?;
    let world_path = saves.join(world_folder_name);
    if !world_path.is_dir() {
        return Err(Error::WorldNotFound {
            instance_id: instance_id.into(),
            folder_name: world_folder_name.into(),
        });
    }
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
fn pick_unused_filename(backups_dir: &std::path::Path, base: &str) -> Result<(String, PathBuf)> {
    for i in 1..=99 {
        let filename = if i == 1 {
            format!("{base}.zip")
        } else {
            format!("{base}.{i}.zip")
        };
        let p = backups_dir.join(&filename);
        if !p.exists() {
            return Ok((filename, p));
        }
    }
    Err(Error::WorldNameUnresolvable {
        folder_name: base.into(),
    })
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
}

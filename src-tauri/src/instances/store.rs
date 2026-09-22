//! Atomic read/write for `instance.json` and `app.json`.
//!
//! Writes go to `<target>.tmp` first, then `rename()` atomically replaces
//! the original. Reads return `Err(Error::Io)` with the path on failure,
//! same shape as the rest of the codebase.

use crate::error::{Error, Result};
use crate::instances::schema::{AppFile, InstanceFile};
use std::path::Path;

/// Read `instance.json` from disk. Errors with `Io` on missing or
/// malformed file (the caller — `scan::list_all` — converts this to
/// "skip with warning"; direct callers like `get_instance` propagate).
pub fn read_instance_json(path: &Path) -> Result<InstanceFile> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| Error::io(path.display().to_string(), e))?;
    let mut file: InstanceFile = serde_json::from_str(&raw)
        .map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}")))?;
    // The DIRECTORY NAME is the id; the `id` field on disk is vestigial and
    // ignored on read. Deriving it here — the single read chokepoint — makes
    // divergence structurally impossible for `scan::list_all` and every direct
    // reader alike, so a folder renamed outside the launcher keeps working
    // instead of becoming a zombie that relaunches empty and refuses to delete.
    //
    // Every caller passes `<instances>/<id>/instance.json`. If the parent name is
    // ever unreadable as UTF-8 we keep the deserialised value, degrading to the
    // previous behaviour rather than producing an empty id.
    if let Some(dir_name) = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
    {
        file.id = dir_name.to_string();
    }
    Ok(file)
}

pub fn write_instance_json(path: &Path, value: &InstanceFile) -> Result<()> {
    write_atomic(path, value)
}

/// Read `app.json`. Missing file → return `AppFile::default()` (callers
/// treat it as "no migration yet"). Malformed → `Err`.
pub fn read_app_json(path: &Path) -> Result<AppFile> {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|e| Error::io(path.display().to_string(), format!("parse: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppFile::default()),
        Err(e) => Err(Error::io(path.display().to_string(), e)),
    }
}

pub fn write_app_json(path: &Path, value: &AppFile) -> Result<()> {
    write_atomic(path, value)
}

/// One writer at a time across every read-modify-write of `app.json`. Holders
/// are synchronous file IO only — nothing awaits inside — and a closure must
/// not call `update_app_json` / `replace_app_json` again, nor any function
/// that does: a `std::sync::Mutex` re-locked on its own thread is not defined
/// (`tests/structural_app_json_rmw.rs` pins it). Innermost lock: the GPU
/// transition (`gpu_pref::LOCK`) runs after the guard drops.
///
/// Poison is recovered on purpose. The mutex guards `()`; the file is always
/// consistent through tmp + rename, so a panic inside one closure leaves
/// nothing torn — and refusing every later write for the session would brick
/// tour completion, active-instance repair and the dismissals over it.
static APP_JSON_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// What a read-modify-write closure decided.
pub enum Verdict {
    Write,
    Unchanged,
}

/// Read (absent = defaults; corrupt = `Err`, nothing written) → `f` → write on
/// `Verdict::Write`, under `APP_JSON_LOCK`. Returns the file as it is after
/// the call, written or not.
pub fn update_app_json(path: &Path, f: impl FnOnce(&mut AppFile) -> Verdict) -> Result<AppFile> {
    let _guard = APP_JSON_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut file = read_app_json(path)?;
    if let Verdict::Write = f(&mut file) {
        write_app_json(path, &file)?;
    }
    Ok(file)
}

/// Write a whole file without reading — for the two sites that mean it: the
/// first-run seed and the URL-scheme flag clear, which must overwrite a file
/// it could not read. Under the same lock.
pub fn replace_app_json(path: &Path, value: &AppFile) -> Result<()> {
    let _guard = APP_JSON_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    write_app_json(path, value)
}

fn write_atomic<T: serde::Serialize>(target: &Path, value: &T) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| Error::io(target.display().to_string(), "no parent dir"))?;
    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    let tmp = target.with_extension("tmp");
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| Error::io(target.display().to_string(), format!("serialize: {e}")))?;
    std::fs::write(&tmp, json).map_err(|e| Error::io(tmp.display().to_string(), e))?;
    std::fs::rename(&tmp, target).map_err(|e| Error::io(target.display().to_string(), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_reads_defaults_when_absent_and_writes_only_on_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        let file = update_app_json(&path, |af| {
            assert!(af.active_instance.is_none());
            Verdict::Unchanged
        })
        .unwrap();
        assert!(file.active_instance.is_none());
        assert!(
            matches!(std::fs::metadata(&path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        );
        let file = update_app_json(&path, |af| {
            af.active_instance = Some("x".into());
            Verdict::Write
        })
        .unwrap();
        assert_eq!(file.active_instance.as_deref(), Some("x"));
        assert_eq!(
            read_app_json(&path).unwrap().active_instance.as_deref(),
            Some("x")
        );
    }

    #[test]
    fn update_refuses_a_corrupt_file_and_leaves_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        std::fs::write(&path, b"{").unwrap();
        let mut ran = false;
        assert!(update_app_json(&path, |_| {
            ran = true;
            Verdict::Write
        })
        .is_err());
        assert!(!ran);
        assert_eq!(std::fs::read(&path).unwrap(), b"{");
    }

    #[test]
    fn replace_writes_without_reading() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        std::fs::write(&path, b"{").unwrap(); // corrupt: replace must not care
        let af = AppFile {
            changelog_seen_version: Some("1".into()),
            ..AppFile::default()
        };
        replace_app_json(&path, &af).unwrap();
        assert_eq!(
            read_app_json(&path)
                .unwrap()
                .changelog_seen_version
                .as_deref(),
            Some("1")
        );
    }

    /// Proven meaningful by a mutation check (PR body): with the guard line
    /// removed from `update_app_json`, this test fails within a few runs.
    #[test]
    fn two_writers_under_the_lock_lose_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::sync::Arc::new(dir.path().join("app.json"));
        let bump = |path: std::sync::Arc<std::path::PathBuf>, field: &'static str| {
            std::thread::spawn(move || {
                for _ in 0..200 {
                    update_app_json(&path, |af| {
                        let v = match field {
                            "a" => &mut af.changelog_seen_version,
                            _ => &mut af.update_dismissed_version,
                        };
                        let n: u32 = v.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
                        *v = Some((n + 1).to_string());
                        Verdict::Write
                    })
                    .unwrap();
                }
            })
        };
        let a = bump(path.clone(), "a");
        let b = bump(path.clone(), "b");
        a.join().unwrap();
        b.join().unwrap();
        let file = read_app_json(&path).unwrap();
        assert_eq!(file.changelog_seen_version.as_deref(), Some("200"));
        assert_eq!(file.update_dismissed_version.as_deref(), Some("200"));
    }
    use crate::instances::schema::LoaderKind;
    use tempfile::tempdir;

    fn sample_instance() -> InstanceFile {
        InstanceFile {
            id: "3f4a-bbbb".into(),
            uid: None,
            name: "Default".into(),
            mc_version: "1.20.4".into(),
            loader: LoaderKind::Vanilla,
            loader_version: None,
            max_heap_mb: 2048,
            min_heap_mb: None,
            extra_jvm_args: String::new(),
            created_unix_ms: 1_700_000_000_000.0,
            mrpack_name: None,
            mrpack_version: None,
            mrpack_project_id: None,
            mrpack_source: None,
            mrpack_summary: None,
            mrpack_version_id: None,
            integrity: None,
            imported_from: None,
            created_from_server: None,
            handled_log_sig: None,
        }
    }

    #[test]
    fn write_then_read_instance_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("3f4a-bbbb/instance.json");
        let value = sample_instance();
        write_instance_json(&path, &value).unwrap();
        let back = read_instance_json(&path).unwrap();
        assert_eq!(value, back);
    }

    #[test]
    fn read_derives_id_from_the_directory_not_the_json() {
        // Regression test for the zombie-instance class. Before this, renaming an
        // instance folder in Explorer left an instance that still listed, launched
        // into an EMPTY recreated directory (losing the user's mods and worlds as
        // far as they could tell), and silently refused to delete.
        let dir = tempdir().unwrap();
        let renamed = dir.path().join("Renamed-By-Hand");
        std::fs::create_dir_all(&renamed).unwrap();
        let path = renamed.join("instance.json");

        let mut value = sample_instance();
        value.id = "the-old-name".into();
        write_instance_json(&path, &value).unwrap();

        let back = read_instance_json(&path).unwrap();
        assert_eq!(back.id, "Renamed-By-Hand");
    }

    #[test]
    fn write_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        // Two levels of missing parent.
        let path = dir.path().join("nested/x/y/instance.json");
        write_instance_json(&path, &sample_instance()).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn write_is_atomic_no_tmp_leak_on_success() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("inst/instance.json");
        write_instance_json(&path, &sample_instance()).unwrap();
        let tmp = path.with_extension("tmp");
        assert!(!tmp.exists(), "temp file should be renamed away");
    }

    #[test]
    fn read_missing_instance_json_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope/instance.json");
        let err = read_instance_json(&path).unwrap_err();
        match err {
            Error::Io { .. } => {}
            other => panic!("expected Io, got {other:?}"),
        }
    }

    #[test]
    fn read_malformed_instance_json_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("inst/instance.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not json").unwrap();
        let err = read_instance_json(&path).unwrap_err();
        match err {
            Error::Io { details, .. } => assert!(details.contains("parse")),
            other => panic!("expected Io, got {other:?}"),
        }
    }

    #[test]
    fn read_missing_app_json_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.json");
        let value = read_app_json(&path).unwrap();
        assert_eq!(value, AppFile::default());
    }

    #[test]
    fn write_then_read_app_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.json");
        let value = AppFile {
            active_instance: Some("3f4a-bbbb".into()),
            ..AppFile::default()
        };
        write_app_json(&path, &value).unwrap();
        let back = read_app_json(&path).unwrap();
        assert_eq!(value, back);
    }

    #[test]
    fn write_then_read_app_with_onboarding_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.json");
        let value = AppFile {
            active_instance: Some("3f4a-bbbb".into()),
            onboarding: crate::instances::schema::OnboardingState {
                tour_completed_version: Some("0.5.0".into()),
            },
            general: crate::instances::schema::GeneralSettings::default(),
            update_dismissed_version: None,
            changelog_seen_version: None,
        };
        write_app_json(&path, &value).unwrap();
        let back = read_app_json(&path).unwrap();
        assert_eq!(value, back);
    }

    #[test]
    fn read_legacy_app_json_without_onboarding_yields_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.json");
        // Legacy format: only version + active_instance, no onboarding key.
        std::fs::write(&path, r#"{"version":1,"active_instance":"3f4a-bbbb"}"#).unwrap();
        let back = read_app_json(&path).unwrap();
        assert_eq!(back.onboarding.tour_completed_version, None);
    }

    #[test]
    fn read_app_json_general_without_ttl_field_defaults_to_seven() {
        // A `general` block written before mod_metadata_ttl_days existed must
        // deserialize the field to the 7-day default, not 0 (which would mean
        // "never expire").
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.json");
        std::fs::write(
            &path,
            r#"{"active_instance":"x","general":{"theme":"dark"}}"#,
        )
        .unwrap();
        let back = read_app_json(&path).unwrap();
        assert_eq!(back.general.mod_metadata_ttl_days, 7);
    }

    #[test]
    fn read_app_json_general_without_concurrency_field_defaults_to_four() {
        // A `general` block written before sftp_upload_concurrency existed must
        // deserialize the field to the 4-stream default via #[serde(default)],
        // not fail the whole AppFile parse.
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.json");
        std::fs::write(
            &path,
            r#"{"active_instance":"x","general":{"theme":"dark"}}"#,
        )
        .unwrap();
        let back = read_app_json(&path).unwrap();
        assert_eq!(back.general.sftp_upload_concurrency, 4);
    }

    #[test]
    fn write_default_onboarding_omits_key_or_writes_none() {
        // Either skip_serializing_if=None or writing `null` is acceptable.
        // This test just ensures roundtrip is clean and the deserialised
        // value is None.
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.json");
        let value = AppFile::default();
        write_app_json(&path, &value).unwrap();
        let back = read_app_json(&path).unwrap();
        assert_eq!(back.onboarding.tour_completed_version, None);
    }

    #[test]
    fn set_active_instance_preserves_onboarding_state() {
        // Regression for the wipe bug found in code-quality review of
        // Task 2. The fix is in instances::mod.rs and instances::migrate.rs
        // (read-modify-write). This test exercises the round-trip directly
        // via read/write to avoid bringing the AppHandle into the unit
        // test surface.
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.json");
        // Seed: user completed the tour and has an active instance.
        let initial = AppFile {
            active_instance: Some("old-instance".into()),
            onboarding: crate::instances::schema::OnboardingState {
                tour_completed_version: Some("0.5.0".into()),
            },
            general: crate::instances::schema::GeneralSettings::default(),
            update_dismissed_version: None,
            changelog_seen_version: None,
        };
        write_app_json(&path, &initial).unwrap();
        // Simulate set_active_instance's read-modify-write.
        let mut current = read_app_json(&path).unwrap();
        current.active_instance = Some("new-instance".into());
        write_app_json(&path, &current).unwrap();
        // Verify tour_completed_version survived.
        let back = read_app_json(&path).unwrap();
        assert_eq!(back.active_instance, Some("new-instance".into()));
        assert_eq!(back.onboarding.tour_completed_version, Some("0.5.0".into()));
    }
}

//! One-shot startup migration. Run from `lib.rs` setup hook before
//! any commands are exposed. Idempotent — exits early if `app.json`
//! already exists.
//!
//! Five scenarios:
//! 1. `app.json` present → noop.
//! 2. No legacy `instances/default/` → seed an empty "Default"
//!    instance with `mc_version=""` (fresh install).
//! 3. Legacy `default/` + `versions/` has vanilla releases → rename
//!    legacy dir to UUID, write `instance.json` with the newest-mtime
//!    release as `mc_version`.
//! 4. Legacy `default/` + `versions/` empty → migrate with `mc_version=""`.
//! 5. Legacy `default/` + `versions/` has only synthetic IDs (Fabric
//!    profiles etc.) → migrate with `mc_version=""` (synth IDs are
//!    not what the UI wants in this field; the user can re-pick).

use crate::error::{Error, Result};
use crate::instances::ids::new_id;
use crate::instances::schema::{
    AppFile, GeneralSettings, InstanceFile, LoaderKind, OnboardingState,
};
use crate::instances::store::{read_app_json, write_app_json, write_instance_json};
use crate::paths::{app_dir, app_file, instance_json};
use std::path::Path;
use std::time::SystemTime;

/// Idempotent: noop if `app.json` already exists. Otherwise creates one
/// instance (migrating legacy `default/` if present, else seeding empty).
pub fn migrate_or_seed(app: &tauri::AppHandle) -> Result<()> {
    let app_root = app_dir(app).map_err(|e| Error::io("<app_dir>", e))?;
    let app_file_path = app_file(app).map_err(|e| Error::io("<app_file>", e))?;

    // Scenario 1.
    if app_file_path.exists() {
        // Even if app.json exists, repair `active_instance` if it points at
        // nothing on disk. This is also done lazily by `get_active_instance`,
        // but doing it here keeps the launcher consistent on startup.
        if let Ok(mut existing) = read_app_json(&app_file_path) {
            if let Some(active) = existing.active_instance.clone() {
                let p = app_root
                    .join("instances")
                    .join(&active)
                    .join("instance.json");
                if !p.exists() {
                    existing.active_instance = None;
                    let _ = write_app_json(&app_file_path, &existing);
                }
            }
        }
        return Ok(());
    }

    let instances_dir = app_root.join("instances");
    let legacy_default = instances_dir.join("default");
    std::fs::create_dir_all(&instances_dir)
        .map_err(|e| Error::io(instances_dir.display().to_string(), e))?;

    let id = new_id();
    let now_ms = unix_ms_f64();
    let mc_version = best_guess_mc_version(&app_root.join("versions")).unwrap_or_default();

    if legacy_default.is_dir() {
        // Scenario 3/4/5.
        let target = instances_dir.join(&id);
        std::fs::rename(&legacy_default, &target)
            .map_err(|e| Error::io(target.display().to_string(), e))?;
    } else {
        // Scenario 2 — fresh install.
        let target = instances_dir.join(&id);
        std::fs::create_dir_all(target.join(".minecraft"))
            .map_err(|e| Error::io(target.display().to_string(), e))?;
    }

    let inst = InstanceFile {
        version: 1,
        id: id.clone(),
        name: "Default".into(),
        mc_version,
        loader: LoaderKind::Vanilla,
        loader_version: None,
        max_heap_mb: 2048,
        extra_jvm_args: String::new(),
        created_unix_ms: now_ms,
        mrpack_name: None,
        mrpack_version: None,
        mrpack_project_id: None,
        mrpack_source: None,
        mrpack_summary: None,
        mrpack_version_id: None,
        integrity: None,
    };
    write_instance_json(
        &instance_json(app, &id).map_err(|e| Error::io("<instance_json>", e))?,
        &inst,
    )?;
    write_app_json(
        &app_file_path,
        &AppFile {
            version: 1,
            active_instance: Some(id),
            onboarding: OnboardingState::default(),
            general: GeneralSettings::default(),
            update_dismissed_version: None,
        },
    )?;
    Ok(())
}

/// Scan `versions_dir` for subdirectories matching the vanilla release
/// format (`^\d+\.\d+(\.\d+)?$`). Return the newest by mtime, or `None`
/// when none match. Synthetic loader profiles (`fabric-loader-...`,
/// `quilt-loader-...`) are filtered out by the regex.
pub fn best_guess_mc_version(versions_dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(versions_dir).ok()?;
    let mut best: Option<(SystemTime, String)> = None;
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let name_os = entry.file_name();
        let Some(name) = name_os.to_str() else {
            continue;
        };
        if !looks_like_release(name) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        match &best {
            Some((t, _)) if *t >= mtime => {}
            _ => best = Some((mtime, name.to_string())),
        }
    }
    best.map(|(_, name)| name)
}

fn looks_like_release(name: &str) -> bool {
    // `^\d+\.\d+(\.\d+)?$` without pulling in the `regex` crate.
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return false;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn unix_ms_f64() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn touch(p: &PathBuf) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"").unwrap();
    }

    #[test]
    fn looks_like_release_accepts_two_and_three_part() {
        assert!(looks_like_release("1.20"));
        assert!(looks_like_release("1.20.4"));
        assert!(looks_like_release("1.21"));
    }

    #[test]
    fn looks_like_release_rejects_snapshots_and_synth_ids() {
        assert!(!looks_like_release(""));
        assert!(!looks_like_release("24w14a"));
        assert!(!looks_like_release("1.20.4-pre1"));
        assert!(!looks_like_release("fabric-loader-0.16.5-1.20.4"));
        assert!(!looks_like_release("1.20."));
        assert!(!looks_like_release(".1.20"));
    }

    #[test]
    fn best_guess_returns_none_for_missing_versions_dir() {
        let dir = tempdir().unwrap();
        assert_eq!(best_guess_mc_version(&dir.path().join("nope")), None);
    }

    #[test]
    fn best_guess_returns_none_for_empty_versions_dir() {
        let dir = tempdir().unwrap();
        let v = dir.path().join("versions");
        std::fs::create_dir_all(&v).unwrap();
        assert_eq!(best_guess_mc_version(&v), None);
    }

    #[test]
    fn best_guess_returns_none_for_only_synth_ids() {
        let dir = tempdir().unwrap();
        let v = dir.path().join("versions");
        touch(&v.join("fabric-loader-0.16.5-1.20.4/x.json"));
        touch(&v.join("quilt-loader-0.27.1-1.21/x.json"));
        assert_eq!(best_guess_mc_version(&v), None);
    }

    #[test]
    fn best_guess_picks_newest_release_by_mtime() {
        let dir = tempdir().unwrap();
        let v = dir.path().join("versions");
        std::fs::create_dir_all(v.join("1.19.4")).unwrap();
        // Sleep briefly so the second dir has a strictly newer mtime.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir_all(v.join("1.20.4")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir_all(v.join("1.21")).unwrap();
        // Add a synth ID newer than all releases — should be filtered out.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir_all(v.join("fabric-loader-0.16.5-1.21")).unwrap();

        let got = best_guess_mc_version(&v);
        assert_eq!(got, Some("1.21".to_string()));
    }
}

//! Per-instance installed-mods registry.
//!
//! File: `{instance}/ftlauncher/installed-mods.json`. Schema v1.
//!
//! On every read, the registry is scanned against the actual contents
//! of `{instance}/.minecraft/mods/` so user-placed jars and renamed /
//! deleted files reconcile cleanly. Hand-editing the mods folder is a
//! supported workflow.

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::fs;

use crate::error::Error;
use crate::mods::platform::{InstalledMod, ModSource};

const FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OnDisk {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    mods: Vec<InstalledMod>,
}

fn default_version() -> u32 { FILE_VERSION }

pub fn registry_dir(instance_root: &Path) -> PathBuf {
    instance_root.join("ftlauncher")
}

pub fn registry_path(instance_root: &Path) -> PathBuf {
    registry_dir(instance_root).join("installed-mods.json")
}

pub fn mods_dir(instance_root: &Path) -> PathBuf {
    instance_root.join(".minecraft").join("mods")
}

/// Read the registry from disk and reconcile against the actual `mods/`
/// directory contents. Persists changes if reconciliation modified state.
pub async fn list(instance_root: &Path) -> Result<Vec<InstalledMod>, Error> {
    let mut state = read_or_empty(instance_root).await?;
    let changed = reconcile(instance_root, &mut state).await?;
    if changed {
        write(instance_root, &state).await?;
    }
    Ok(state.mods)
}

async fn read_or_empty(instance_root: &Path) -> Result<OnDisk, Error> {
    let path = registry_path(instance_root);
    if !fs::try_exists(&path).await.map_err(|e| io_err(&path, e))? {
        return Ok(OnDisk { version: FILE_VERSION, mods: vec![] });
    }
    let bytes = fs::read(&path).await.map_err(|e| io_err(&path, e))?;
    // Corrupt JSON: treat as empty; reconcile will rebuild from disk.
    Ok(serde_json::from_slice::<OnDisk>(&bytes).unwrap_or(OnDisk { version: FILE_VERSION, mods: vec![] }))
}

async fn write(instance_root: &Path, state: &OnDisk) -> Result<(), Error> {
    let dir = registry_dir(instance_root);
    fs::create_dir_all(&dir).await.map_err(|e| io_err(&dir, e))?;
    let final_path = registry_path(instance_root);
    let tmp = final_path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|e| Error::ModsDecode { platform: "installed-mods.json".into(), details: e.to_string() })?;
    fs::write(&tmp, &bytes).await.map_err(|e| io_err(&tmp, e))?;
    fs::rename(&tmp, &final_path).await.map_err(|e| io_err(&final_path, e))?;
    Ok(())
}

/// Sync `state.mods` against the contents of `mods/`. Returns true if
/// anything changed (caller persists).
async fn reconcile(instance_root: &Path, state: &mut OnDisk) -> Result<bool, Error> {
    let dir = mods_dir(instance_root);

    // (base_filename, sha1_lower, enabled) for every file on disk.
    // A missing mods/ directory is equivalent to an empty one: any stale
    // JSON entries should still be dropped.
    let mut on_disk: Vec<(String, String, bool)> = Vec::new();
    if fs::try_exists(&dir).await.map_err(|e| io_err(&dir, e))? {
        let mut rd = fs::read_dir(&dir).await.map_err(|e| io_err(&dir, e))?;
        while let Some(entry) = rd.next_entry().await.map_err(|e| io_err(&dir, e))? {
            if !entry.metadata().await.map_err(|e| io_err(&dir, e))?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let (enabled, base_name) = if let Some(stripped) = name.strip_suffix(".disabled") {
                (false, stripped.to_string())
            } else if name.ends_with(".jar") {
                (true, name.clone())
            } else {
                continue;
            };
            let path = entry.path();
            let bytes = fs::read(&path).await.map_err(|e| io_err(&path, e))?;
            let sha = hex::encode(Sha1::digest(&bytes));
            on_disk.push((base_name, sha, enabled));
        }
    }

    let mut changed = false;

    // 1. Update existing JSON entries by SHA, fixing filename / enabled drift.
    for m in state.mods.iter_mut() {
        if let Some((on_disk_name, _, on_disk_enabled)) =
            on_disk.iter().find(|(_, sha, _)| sha.eq_ignore_ascii_case(&m.sha1))
        {
            if m.filename != *on_disk_name {
                m.filename = on_disk_name.clone();
                changed = true;
            }
            if m.enabled != *on_disk_enabled {
                m.enabled = *on_disk_enabled;
                changed = true;
            }
        }
    }

    // 2. Drop JSON entries with no matching file on disk.
    let before = state.mods.len();
    let on_disk_shas: std::collections::HashSet<String> =
        on_disk.iter().map(|(_, s, _)| s.to_ascii_lowercase()).collect();
    state.mods.retain(|m| on_disk_shas.contains(&m.sha1.to_ascii_lowercase()));
    if state.mods.len() != before {
        changed = true;
    }

    // 3. Add synthesized entries for files on disk with no JSON record.
    let known_shas: std::collections::HashSet<String> =
        state.mods.iter().map(|m| m.sha1.to_ascii_lowercase()).collect();
    for (filename, sha, enabled) in on_disk.iter() {
        if !known_shas.contains(&sha.to_ascii_lowercase()) {
            state.mods.push(InstalledMod {
                filename: filename.clone(),
                sha1: sha.clone(),
                source: None,
                project_id: None,
                version_id: None,
                name: filename.clone(),
                version_number: None,
                installed_at: Utc::now().to_rfc3339(),
                enabled: *enabled,
            });
            changed = true;
        }
    }

    Ok(changed)
}

/// Append a new entry. Caller has already placed the file in `mods/`.
pub async fn add(instance_root: &Path, m: InstalledMod) -> Result<(), Error> {
    let mut state = read_or_empty(instance_root).await?;
    state.mods.retain(|x| !x.sha1.eq_ignore_ascii_case(&m.sha1));
    state.mods.push(m);
    write(instance_root, &state).await
}

/// Remove the entry with the given SHA-1.
pub async fn remove(instance_root: &Path, sha1: &str) -> Result<(), Error> {
    let mut state = read_or_empty(instance_root).await?;
    state.mods.retain(|x| !x.sha1.eq_ignore_ascii_case(sha1));
    write(instance_root, &state).await
}

/// Toggle `enabled` for the entry with the given SHA-1.
pub async fn set_enabled(instance_root: &Path, sha1: &str, enabled: bool) -> Result<(), Error> {
    let mut state = read_or_empty(instance_root).await?;
    if let Some(m) = state.mods.iter_mut().find(|x| x.sha1.eq_ignore_ascii_case(sha1)) {
        m.enabled = enabled;
    }
    write(instance_root, &state).await
}

fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::ModsInstancePath { path: path.display().to_string(), details: e.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn place_jar(dir: &Path, name: &str, body: &[u8]) -> String {
        fs::create_dir_all(dir).await.unwrap();
        fs::write(dir.join(name), body).await.unwrap();
        hex::encode(Sha1::digest(body))
    }

    #[tokio::test]
    async fn empty_instance_yields_empty_list() {
        let td = TempDir::new().unwrap();
        let mods = list(td.path()).await.unwrap();
        assert!(mods.is_empty());
    }

    #[tokio::test]
    async fn synthesizes_entry_for_manual_jar() {
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "manual.jar", b"abc").await;
        let mods = list(td.path()).await.unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].sha1, sha);
        assert_eq!(mods[0].filename, "manual.jar");
        assert!(mods[0].source.is_none());
        assert!(mods[0].enabled);
    }

    #[tokio::test]
    async fn disabled_suffix_marks_entry_disabled() {
        let td = TempDir::new().unwrap();
        place_jar(&mods_dir(td.path()), "foo.jar.disabled", b"xyz").await;
        let mods = list(td.path()).await.unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].filename, "foo.jar");
        assert!(!mods[0].enabled);
    }

    #[tokio::test]
    async fn drops_stale_json_entry_when_file_missing() {
        let td = TempDir::new().unwrap();
        let stale = InstalledMod {
            filename: "gone.jar".into(),
            sha1: "0000000000000000000000000000000000000000".into(),
            source: Some(ModSource::Modrinth),
            project_id: Some("zzz".into()),
            version_id: Some("yyy".into()),
            name: "Gone".into(),
            version_number: Some("1.0".into()),
            installed_at: Utc::now().to_rfc3339(),
            enabled: true,
        };
        add(td.path(), stale).await.unwrap();
        let mods = list(td.path()).await.unwrap();
        assert!(mods.is_empty());
    }

    #[tokio::test]
    async fn add_then_list_round_trips_metadata() {
        let td = TempDir::new().unwrap();
        let sha = place_jar(&mods_dir(td.path()), "jei.jar", b"jei-bytes").await;
        add(td.path(), InstalledMod {
            filename: "jei.jar".into(),
            sha1: sha.clone(),
            source: Some(ModSource::Modrinth),
            project_id: Some("u6dRKJwZ".into()),
            version_id: Some("ZG8XHvO0".into()),
            name: "Just Enough Items".into(),
            version_number: Some("15.2.0.27".into()),
            installed_at: Utc::now().to_rfc3339(),
            enabled: true,
        }).await.unwrap();
        let mods = list(td.path()).await.unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].name, "Just Enough Items");
        assert_eq!(mods[0].source, Some(ModSource::Modrinth));
    }

    #[tokio::test]
    async fn corrupt_json_rebuilds_from_disk() {
        let td = TempDir::new().unwrap();
        place_jar(&mods_dir(td.path()), "rebuilt.jar", b"data").await;
        let dir = registry_dir(td.path());
        fs::create_dir_all(&dir).await.unwrap();
        fs::write(registry_path(td.path()), b"this is not json").await.unwrap();
        let mods = list(td.path()).await.unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].filename, "rebuilt.jar");
    }
}

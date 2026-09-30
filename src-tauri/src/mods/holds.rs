//! Per-instance "don't update" list — projects the user put on hold (D9).
//!
//! File: `{instance}/lucerna/holds.json`, `{ "held": [{ "source", "project_id" }] }`.
//! Keyed by PROJECT, not by jar: a hold survives updates, version switches and
//! trash restores by construction, and `InstalledMod` stays untouched.
//!
//! Fallback direction: absent → nothing held. Unreadable or unparseable → an
//! ERROR, never "nothing held": that answer would offer an update the user said
//! no to, and a write over it would erase their other holds.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::error::Error;
use crate::mods::platform::ModSource;

const FILE_NAME: &str = "holds.json";
/// A unique temp name per write — the `installed::write` shape.
static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
pub struct HeldProject {
    pub source: ModSource,
    pub project_id: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OnDisk {
    #[serde(default)]
    held: Vec<HeldProject>,
}

/// Serialises `set`'s read-modify-write. One lock for every instance: a hold is
/// a click. Not `registry_lock` — that one is the two registries' own
/// (`tests/structural_registry_rmw_lock.rs`, rule 3).
fn holds_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub fn holds_path(instance_root: &Path) -> PathBuf {
    crate::mods::installed::registry_dir(instance_root).join(FILE_NAME)
}

pub fn is_held(holds: &[HeldProject], source: ModSource, project_id: &str) -> bool {
    holds
        .iter()
        .any(|h| h.source == source && h.project_id == project_id)
}

pub async fn load(instance_root: &Path) -> Result<Vec<HeldProject>, Error> {
    let path = holds_path(instance_root);
    let bytes = match fs::read(&path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(Error::ModsInstancePath {
                path: path.display().to_string(),
                details: e.to_string(),
            })
        }
    };
    serde_json::from_slice::<OnDisk>(&bytes)
        .map(|d| d.held)
        .map_err(|e| Error::ModsDecode {
            platform: "holds.json".into(),
            details: e.to_string(),
        })
}

/// Hold (`true`) or release (`false`) one project. Idempotent; sorted on disk.
pub async fn set(instance_root: &Path, project: HeldProject, hold: bool) -> Result<(), Error> {
    let _guard = holds_lock().lock().await;
    let mut held = load(instance_root).await?;
    held.retain(|h| h != &project);
    if hold {
        held.push(project);
    }
    held.sort_by(|a, b| (a.source as u8, &a.project_id).cmp(&(b.source as u8, &b.project_id)));
    save(instance_root, &OnDisk { held })
        .await
        .map_err(|e| Error::ModsInstancePath {
            path: holds_path(instance_root).display().to_string(),
            details: e.to_string(),
        })
}

/// Temp-then-rename, the `hash_cache::save` shape.
async fn save(instance_root: &Path, state: &OnDisk) -> std::io::Result<()> {
    let dir = crate::mods::installed::registry_dir(instance_root);
    fs::create_dir_all(&dir).await?;
    let final_path = holds_path(instance_root);
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = final_path.with_extension(format!("json.tmp.{}.{seq}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?;
    fs::write(&tmp, &bytes).await?;
    fs::rename(&tmp, &final_path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn held(source: ModSource, id: &str) -> HeldProject {
        HeldProject {
            source,
            project_id: id.into(),
        }
    }

    #[tokio::test]
    async fn holds_round_trip_through_the_sidecar_sorted() {
        let td = TempDir::new().unwrap();
        set(td.path(), held(ModSource::Modrinth, "zz"), true)
            .await
            .unwrap();
        set(td.path(), held(ModSource::Curseforge, "123"), true)
            .await
            .unwrap();
        set(td.path(), held(ModSource::Modrinth, "aa"), true)
            .await
            .unwrap();
        assert_eq!(
            holds_path(td.path()),
            td.path().join("lucerna").join("holds.json")
        );
        assert_eq!(
            load(td.path()).await.unwrap(),
            vec![
                held(ModSource::Modrinth, "aa"),
                held(ModSource::Modrinth, "zz"),
                held(ModSource::Curseforge, "123"),
            ]
        );
    }

    #[tokio::test]
    async fn holding_twice_is_one_hold_and_releasing_touches_only_that_project() {
        let td = TempDir::new().unwrap();
        for _ in 0..2 {
            set(td.path(), held(ModSource::Modrinth, "a"), true)
                .await
                .unwrap();
        }
        set(td.path(), held(ModSource::Modrinth, "b"), true)
            .await
            .unwrap();
        set(td.path(), held(ModSource::Modrinth, "a"), false)
            .await
            .unwrap();
        assert_eq!(
            load(td.path()).await.unwrap(),
            vec![held(ModSource::Modrinth, "b")]
        );
    }

    #[tokio::test]
    async fn no_file_is_no_holds_but_a_corrupt_one_is_an_error_not_a_blank_slate() {
        let td = TempDir::new().unwrap();
        assert!(load(td.path()).await.unwrap().is_empty());
        tokio::fs::create_dir_all(holds_path(td.path()).parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(holds_path(td.path()), b"{ not json")
            .await
            .unwrap();
        assert!(load(td.path()).await.is_err());
        assert!(
            set(td.path(), held(ModSource::Modrinth, "a"), true)
                .await
                .is_err(),
            "never written over what could not be read"
        );
        assert_eq!(
            tokio::fs::read(holds_path(td.path())).await.unwrap(),
            b"{ not json"
        );
    }

    #[test]
    fn a_hold_matches_source_and_project_together() {
        let holds = vec![held(ModSource::Modrinth, "p")];
        assert!(is_held(&holds, ModSource::Modrinth, "p"));
        assert!(!is_held(&holds, ModSource::Curseforge, "p"));
        assert!(!is_held(&holds, ModSource::Modrinth, "q"));
    }
}

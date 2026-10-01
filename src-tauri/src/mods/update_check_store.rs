//! The last mod-update check of an instance, persisted (D9).
//!
//! File: `{instance}/lucerna/update-check.json`. One source of truth for
//! «проверено …» and the update badges (replaces the frontend's session LRU), so
//! they cannot disagree. Written by every `mods_check_updates`; read by
//! `mods_last_update_check`, which keeps rows only for jars still installed and
//! projects not on hold.
//!
//! Fallback: absent → "not checked". Unreadable, unparseable or another schema
//! version → also "not checked", LOGGED — a half-read answer must never paint
//! badges. A failed write is logged by the caller and costs only persistence.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::mods::holds::HeldProject;
use crate::mods::platform::InstalledMod;
use crate::mods::updates::ModUpdateCheck;

const FILE_VERSION: u32 = 1;
const FILE_NAME: &str = "update-check.json";
/// A unique temp name per write — the `installed::write` shape.
static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct StoredUpdateCheck {
    /// Unix seconds. `u32`: specta forbids 64-bit integers and an `f64` reaches
    /// TS as `number | null`. Fits until 2106.
    pub checked_at_secs: u32,
    pub results: Vec<ModUpdateCheck>,
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    version: u32,
    checked_at_secs: u32,
    results: Vec<ModUpdateCheck>,
}

pub fn store_path(instance_root: &Path) -> PathBuf {
    crate::mods::installed::registry_dir(instance_root).join(FILE_NAME)
}

/// Now, in the unit [`StoredUpdateCheck::checked_at_secs`] carries. A clock
/// before 1970 reads as 0; one past 2106 saturates.
pub fn now_secs() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
        .unwrap_or(0)
}

/// The stored check, or `None` = "not checked": absent, unreadable, corrupt or
/// written by another schema version — the last three logged.
pub async fn load(instance_root: &Path) -> Option<StoredUpdateCheck> {
    let path = store_path(instance_root);
    let bytes = match fs::read(&path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            crate::diag!(
                "mods: stored update check unreadable ({}): {e}",
                path.display()
            );
            return None;
        }
    };
    match serde_json::from_slice::<OnDisk>(&bytes) {
        Ok(d) if d.version == FILE_VERSION => Some(StoredUpdateCheck {
            checked_at_secs: d.checked_at_secs,
            results: d.results,
        }),
        Ok(d) => {
            crate::diag!(
                "mods: stored update check has version {} ({}) — ignored",
                d.version,
                path.display()
            );
            None
        }
        Err(e) => {
            crate::diag!(
                "mods: stored update check corrupt ({}): {e}",
                path.display()
            );
            None
        }
    }
}

/// Temp-then-rename, the `hash_cache::save` shape. The caller logs a failure.
pub(crate) async fn save(instance_root: &Path, check: &StoredUpdateCheck) -> std::io::Result<()> {
    let dir = crate::mods::installed::registry_dir(instance_root);
    fs::create_dir_all(&dir).await?;
    let final_path = store_path(instance_root);
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = final_path.with_extension(format!("json.tmp.{}.{seq}", std::process::id()));
    let on_disk = OnDisk {
        version: FILE_VERSION,
        checked_at_secs: check.checked_at_secs,
        results: check.results.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&on_disk).map_err(std::io::Error::other)?;
    fs::write(&tmp, &bytes).await?;
    fs::rename(&tmp, &final_path).await
}

/// The stored check as it applies NOW: rows for jars still installed (an update
/// changes the sha1, so its old row drops out) and projects not on hold.
pub fn current_rows(
    stored: StoredUpdateCheck,
    installed: &[InstalledMod],
    holds: &[HeldProject],
) -> StoredUpdateCheck {
    let results = stored
        .results
        .into_iter()
        .filter(|r| {
            installed
                .iter()
                .any(|m| m.sha1.eq_ignore_ascii_case(&r.sha1))
        })
        .filter(|r| !crate::mods::holds::is_held(holds, r.source, &r.project_id))
        .collect();
    StoredUpdateCheck {
        checked_at_secs: stored.checked_at_secs,
        results,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::holds::HeldProject;
    use crate::mods::platform::{LoaderKind, ModFile, ModSource, ModVersion};
    use crate::mods::updates::ModUpdateState;
    use tempfile::TempDir;

    fn check(sha1: &str, project: &str, state: ModUpdateState) -> ModUpdateCheck {
        ModUpdateCheck {
            sha1: sha1.into(),
            name: project.to_uppercase(),
            source: ModSource::Modrinth,
            project_id: project.into(),
            current_version_id: format!("{project}-v1"),
            current_version_number: Some("1.0".into()),
            state,
        }
    }

    fn target(project: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: project.into(),
            version_id: format!("{project}-v2"),
            name: "2.0".into(),
            version_number: "2.0".into(),
            mc_versions: vec!["1.21.1".into()],
            loaders: vec![LoaderKind::NeoForge],
            primary_file: ModFile {
                filename: format!("{project}-2.jar"),
                url: format!("https://cdn.modrinth.com/{project}-2.jar"),
                sha1: Some("bb".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: Vec::new(),
            published_at: None,
        }
    }

    fn row(sha1: &str, project: &str) -> InstalledMod {
        InstalledMod {
            filename: format!("{project}.jar"),
            sha1: sha1.into(),
            source: Some(ModSource::Modrinth),
            project_id: Some(project.into()),
            version_id: Some(format!("{project}-v1")),
            name: project.into(),
            version_number: Some("1.0".into()),
            installed_at: "2026-01-01T00:00:00Z".into(),
            enabled: true,
            enrich_attempted: false,
            requires: Vec::new(),
        }
    }

    #[tokio::test]
    async fn a_saved_check_loads_back_with_its_time_and_states() {
        let td = TempDir::new().unwrap();
        let stored = StoredUpdateCheck {
            checked_at_secs: 1_790_000_000,
            results: vec![
                check(
                    "aa",
                    "p",
                    ModUpdateState::UpdateAvailable {
                        target: target("p"),
                    },
                ),
                check(
                    "bb",
                    "q",
                    ModUpdateState::CheckFailed {
                        reason: "offline".into(),
                    },
                ),
            ],
        };
        save(td.path(), &stored).await.unwrap();
        assert_eq!(
            store_path(td.path()),
            td.path().join("lucerna").join("update-check.json")
        );
        let back = load(td.path()).await.expect("stored");
        assert_eq!(back.checked_at_secs, 1_790_000_000);
        assert_eq!(
            serde_json::to_value(&back.results).unwrap(),
            serde_json::to_value(&stored.results).unwrap()
        );
    }

    #[tokio::test]
    async fn an_absent_corrupt_or_foreign_file_is_not_checked() {
        let td = TempDir::new().unwrap();
        assert!(load(td.path()).await.is_none(), "absent");
        tokio::fs::create_dir_all(store_path(td.path()).parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(store_path(td.path()), b"{ not json")
            .await
            .unwrap();
        assert!(load(td.path()).await.is_none(), "corrupt");
        tokio::fs::write(
            store_path(td.path()),
            br#"{"version":99,"checked_at_secs":1,"results":[]}"#,
        )
        .await
        .unwrap();
        assert!(load(td.path()).await.is_none(), "another schema");
    }

    #[test]
    fn only_rows_for_installed_unheld_jars_are_shown() {
        let stored = StoredUpdateCheck {
            checked_at_secs: 7,
            results: vec![
                check("aa", "p1", ModUpdateState::UpToDate),
                check("bb", "p2", ModUpdateState::UpToDate),
                check("cc", "p3", ModUpdateState::UpToDate),
            ],
        };
        let installed = vec![row("AA", "p1"), row("bb", "p2")]; // cc updated or removed since
        let holds = vec![HeldProject {
            source: ModSource::Modrinth,
            project_id: "p2".into(),
        }];
        let got = current_rows(stored, &installed, &holds);
        assert_eq!(got.checked_at_secs, 7);
        let shas: Vec<&str> = got.results.iter().map(|r| r.sha1.as_str()).collect();
        assert_eq!(shas, ["aa"]);
    }
}

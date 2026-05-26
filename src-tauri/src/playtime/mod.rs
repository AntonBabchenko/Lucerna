use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Type, Default, PartialEq, Eq)]
pub struct PlaytimeStats {
    pub total_seconds: u64,
    pub session_count: u32,
    pub last_session_seconds: u64,
    /// `None` if no session has ever been recorded — lets the UI
    /// distinguish "never played" from "played for 0s, last session
    /// ended at the epoch".
    pub last_session_unix_ms: Option<i64>,
}

const PLAYTIME_FILE: &str = "playtime.json";
const META_DIR: &str = ".ftlauncher";

fn playtime_path(instance_root: &Path) -> std::path::PathBuf {
    instance_root.join(META_DIR).join(PLAYTIME_FILE)
}

/// Read stats for an instance. Missing or malformed file returns zeros
/// rather than erroring — playtime is best-effort and should not block
/// the Overview pane from rendering.
pub fn get_stats_at(instance_root: &Path) -> Result<PlaytimeStats> {
    let path = playtime_path(instance_root);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<PlaytimeStats>(&bytes) {
            Ok(stats) => Ok(stats),
            Err(e) => {
                eprintln!(
                    "playtime: {} malformed ({e}); resetting to zero stats",
                    path.display()
                );
                Ok(PlaytimeStats::default())
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(PlaytimeStats::default()),
        Err(e) => Err(Error::PlaytimeIo {
            details: format!("read {}: {e}", path.display()),
        }),
    }
}

/// Append a completed session of `duration_seconds` length. Creates the
/// file on first call. Negative deltas (clock skew) are clamped to zero
/// — count still ticks because the session did occur.
pub fn record_session_at(instance_root: &Path, duration_seconds: u64) -> Result<()> {
    let mut stats = get_stats_at(instance_root)?;
    stats.total_seconds = stats.total_seconds.saturating_add(duration_seconds);
    stats.session_count = stats.session_count.saturating_add(1);
    stats.last_session_seconds = duration_seconds;
    stats.last_session_unix_ms = Some(chrono::Utc::now().timestamp_millis());

    let path = playtime_path(instance_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::PlaytimeIo {
            details: format!("mkdir {}: {e}", parent.display()),
        })?;
    }
    let json = serde_json::to_vec_pretty(&stats).map_err(|e| Error::PlaytimeIo {
        details: format!("serialise: {e}"),
    })?;
    std::fs::write(&path, json).map_err(|e| Error::PlaytimeIo {
        details: format!("write {}: {e}", path.display()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_instance_root() -> (TempDir, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let inst_root = tmp.path().join("instances").join("test-id");
        std::fs::create_dir_all(&inst_root).unwrap();
        (tmp, inst_root)
    }

    #[test]
    fn record_session_creates_file_on_first_call() {
        let (_tmp, inst) = temp_instance_root();
        record_session_at(&inst, 60).unwrap();
        let stats = get_stats_at(&inst).unwrap();
        assert_eq!(stats.total_seconds, 60);
        assert_eq!(stats.session_count, 1);
        assert_eq!(stats.last_session_seconds, 60);
        assert!(stats.last_session_unix_ms.is_some());
    }

    #[test]
    fn record_session_appends_to_existing() {
        let (_tmp, inst) = temp_instance_root();
        record_session_at(&inst, 100).unwrap();
        record_session_at(&inst, 250).unwrap();
        let stats = get_stats_at(&inst).unwrap();
        assert_eq!(stats.total_seconds, 350);
        assert_eq!(stats.session_count, 2);
        assert_eq!(stats.last_session_seconds, 250);
    }

    #[test]
    fn get_stats_missing_file_returns_zero_with_none_timestamp() {
        let (_tmp, inst) = temp_instance_root();
        let stats = get_stats_at(&inst).unwrap();
        assert_eq!(stats.total_seconds, 0);
        assert_eq!(stats.session_count, 0);
        assert_eq!(stats.last_session_seconds, 0);
        assert_eq!(stats.last_session_unix_ms, None);
    }

    #[test]
    fn get_stats_malformed_file_returns_zero_stats() {
        let (_tmp, inst) = temp_instance_root();
        let path = inst.join(".ftlauncher").join("playtime.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not json {{").unwrap();
        let stats = get_stats_at(&inst).unwrap();
        assert_eq!(stats.total_seconds, 0);
        assert_eq!(stats.session_count, 0);
    }
}

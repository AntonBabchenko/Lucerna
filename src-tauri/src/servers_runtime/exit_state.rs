//! Persisted last-exit code for a server, written by the exit watcher when a
//! server process stops. Lets `server_list` show a crash-vs-clean state
//! (`last_exit_code`) without keeping the launcher running. Tiny single-int file.

use std::path::Path;

const FILE: &str = "last-exit";

/// Persist the process exit `code` under `<runtime>/last-exit`. Best-effort.
pub fn write(runtime: &Path, code: i32) {
    let _ = std::fs::create_dir_all(runtime);
    let _ = std::fs::write(runtime.join(FILE), code.to_string());
}

/// Remove the recorded exit code (called at start so a stale crash code can't
/// appear next to a now-running server). Best-effort.
pub fn clear(runtime: &Path) {
    let _ = std::fs::remove_file(runtime.join(FILE));
}

/// Read the recorded last-exit code, or `None` if absent/unparseable.
pub fn read(runtime: &Path) -> Option<i32> {
    std::fs::read_to_string(runtime.join(FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()), None);
        write(dir.path(), 1);
        assert_eq!(read(dir.path()), Some(1));
        // A clean exit (0) is recorded distinctly from "never run" (None).
        write(dir.path(), 0);
        assert_eq!(read(dir.path()), Some(0));
    }

    #[test]
    fn read_none_on_garbage() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "xyz").unwrap();
        assert_eq!(read(dir.path()), None);
    }
}

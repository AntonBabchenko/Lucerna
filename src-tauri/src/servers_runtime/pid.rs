//! The per-server runtime PID file. Lets `server_list` reconcile live OS
//! processes after a launcher restart (Bug A part 2) and lets the diagnoser
//! offer "stop the leftover process" for a world that is still locked.

use std::path::Path;

/// Write `pid` to `<runtime>/server.pid` atomically (tmp + rename).
pub fn write_pid(pid_file: &Path, pid: u32) -> std::io::Result<()> {
    if let Some(parent) = pid_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = pid_file.with_extension("pid.tmp");
    std::fs::write(&tmp, pid.to_string())?;
    std::fs::rename(&tmp, pid_file)
}

/// Read the recorded PID, or `None` if the file is absent/unparseable.
pub fn read_pid(pid_file: &Path) -> Option<u32> {
    std::fs::read_to_string(pid_file).ok()?.trim().parse().ok()
}

/// Remove the PID file. An absent file is success.
pub fn clear_pid(pid_file: &Path) {
    let _ = std::fs::remove_file(pid_file);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_read_clear_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("runtime/server.pid");
        assert_eq!(read_pid(&f), None);
        write_pid(&f, 4321).unwrap();
        assert_eq!(read_pid(&f), Some(4321));
        clear_pid(&f);
        assert_eq!(read_pid(&f), None);
    }

    #[test]
    fn read_pid_none_on_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("server.pid");
        std::fs::write(&f, "not-a-pid").unwrap();
        assert_eq!(read_pid(&f), None);
    }
}

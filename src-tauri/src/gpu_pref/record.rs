//! The record of what Lucerna wrote to the OS GPU store: one entry per exe,
//! with the field it wrote and what was there before. Lives beside the
//! data-location pointer — per user and per machine, like the HKCU hive it
//! describes — never in the data root.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

pub const FILE: &str = "gpu-preferences.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub exe: String,
    /// The `GpuPreference` field Lucerna wrote ("1" / "2").
    pub written: String,
    /// The field before Lucerna's first write; `None` = it was absent.
    pub previous: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    #[serde(default)]
    pub entries: Vec<Entry>,
}

/// What a read found. A corrupt file is NOT an empty one: nothing may be
/// touched on the strength of a record that cannot be read.
#[derive(Debug)]
pub enum Read {
    Absent,
    Present(Record),
}

pub fn read(path: &Path) -> io::Result<Read> {
    // RED STUB (push 1): everything reads as absent.
    let _ = path;
    Ok(Read::Absent)
}

/// Atomic replace: temp file beside the target, then rename.
pub fn write(path: &Path, record: &Record) -> io::Result<()> {
    // RED STUB (push 1): writes nothing.
    let _ = (path, record);
    Ok(())
}

impl Record {
    pub fn find(&self, exe: &Path) -> Option<&Entry> {
        self.entries.iter().find(|e| Path::new(&e.exe) == exe)
    }

    pub fn upsert(&mut self, entry: Entry) {
        // RED STUB (push 1): appends without replacing.
        self.entries.push(entry);
    }

    pub fn remove(&mut self, exe: &str) {
        // RED STUB (push 1).
        let _ = exe;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(exe: &str, written: &str, previous: Option<&str>) -> Entry {
        Entry {
            exe: exe.into(),
            written: written.into(),
            previous: previous.map(str::to_owned),
        }
    }

    #[test]
    fn absent_file_reads_as_absent_not_as_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            read(&dir.path().join(FILE)).unwrap(),
            Read::Absent
        ));
    }

    #[test]
    fn round_trips_and_upserts_by_exe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let mut rec = Record::default();
        rec.upsert(entry("C:/a/javaw.exe", "2", Some("1")));
        rec.upsert(entry("C:/b/javaw.exe", "1", None));
        rec.upsert(entry("C:/a/javaw.exe", "1", Some("1"))); // replaces, does not duplicate
        write(&path, &rec).unwrap();
        let Read::Present(back) = read(&path).unwrap() else {
            panic!("expected a record");
        };
        assert_eq!(back, rec);
        assert_eq!(back.entries.len(), 2);
        assert_eq!(
            back.find(Path::new("C:/a/javaw.exe"))
                .map(|e| e.written.as_str()),
            Some("1")
        );
    }

    #[test]
    fn remove_drops_one_exe() {
        let mut rec = Record::default();
        rec.upsert(entry("C:/a/javaw.exe", "2", None));
        rec.upsert(entry("C:/b/javaw.exe", "2", None));
        rec.remove("C:/a/javaw.exe");
        assert_eq!(rec.entries.len(), 1);
        assert_eq!(rec.entries[0].exe, "C:/b/javaw.exe");
    }

    #[test]
    fn a_corrupt_file_is_an_error_not_an_empty_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        std::fs::write(&path, b"{").unwrap();
        let err = read(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}

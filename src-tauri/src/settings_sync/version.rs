//! What an instance's Minecraft version allows to sync (spec §B4–§B6).

use std::path::Path;

use serde::Serialize;

use crate::datapacks::compat::{jar_is_stable, world_version_of_jar, JarWorldVersion};

/// The hotbars' gate: the DataVersion the instance's game saves with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DataVersion {
    Version {
        n: i32,
    },
    /// A jar that records no DataVersion (any version before 1.14).
    NotRecorded,
    /// The game files are not installed yet, or the jar is damaged.
    JarUnavailable,
    /// A snapshot or pre-release, or a jar that does not say it is a release:
    /// its DataVersion would raise the gate for every release.
    NotRelease,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionFacts {
    pub mc_version: String,
    pub data: DataVersion,
}

impl VersionFacts {
    /// Reads the client jar: blocking, call it inside `spawn_blocking`.
    pub fn read(versions_dir: &Path, mc_version: &str) -> Self {
        let data = match world_version_of_jar(versions_dir, mc_version) {
            JarWorldVersion::JarUnavailable => DataVersion::JarUnavailable,
            JarWorldVersion::NotRecorded => DataVersion::NotRecorded,
            JarWorldVersion::Version(n) => match jar_is_stable(versions_dir, mc_version) {
                Some(true) => DataVersion::Version { n },
                Some(false) | None => DataVersion::NotRelease,
            },
        };
        Self {
            mc_version: mc_version.to_string(),
            data,
        }
    }

    /// 1.19 and newer know the `hidden` flag; an older game would list hidden
    /// servers. A version name we cannot order counts as new (showing an extra
    /// server is the harmless side).
    pub fn shows_hidden_servers(&self) -> bool {
        at_least(&self.mc_version, (1, 19, 0)) != Some(false)
    }

    /// command_history.txt exists from 1.20.2; a version we cannot order (a
    /// snapshot id) does not sync it.
    pub fn history_supported(&self) -> bool {
        at_least(&self.mc_version, (1, 20, 2)) == Some(true)
    }

    pub fn hotbar_version(&self) -> Option<i32> {
        match self.data {
            DataVersion::Version { n } => Some(n),
            _ => None,
        }
    }
}

/// Is `mc_version` at least `want`? `None` when it is not a plain release
/// number (`24w10a`, `1.20.2-pre1`).
pub(crate) fn at_least(mc_version: &str, want: (u32, u32, u32)) -> Option<bool> {
    let mut parts = [0u32; 3];
    let mut count = 0;
    for (i, piece) in mc_version.trim().split('.').enumerate() {
        if i >= 3 || piece.is_empty() || !piece.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        parts[i] = piece.parse().ok()?;
        count += 1;
    }
    (count >= 2).then(|| (parts[0], parts[1], parts[2]) >= want)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(versions: &Path, mc: &str, version_json: Option<&str>) {
        let dir = versions.join(mc);
        std::fs::create_dir_all(&dir).unwrap();
        let f = std::fs::File::create(dir.join(format!("{mc}.jar"))).unwrap();
        let mut w = zip::ZipWriter::new(f);
        if let Some(j) = version_json {
            w.start_file("version.json", zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(j.as_bytes()).unwrap();
        } else {
            w.start_file(
                "net/minecraft/A.class",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            w.write_all(b"x").unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn release_numbers_are_ordered_and_others_are_not() {
        assert_eq!(at_least("1.20.2", (1, 20, 2)), Some(true));
        assert_eq!(at_least("1.20.1", (1, 20, 2)), Some(false));
        assert_eq!(at_least("1.21", (1, 20, 2)), Some(true));
        assert_eq!(at_least("26.1.1", (1, 20, 2)), Some(true));
        assert_eq!(at_least("1.19", (1, 19, 0)), Some(true));
        assert_eq!(at_least("24w10a", (1, 20, 2)), None);
        assert_eq!(at_least("1.20.2-pre1", (1, 20, 2)), None);
    }

    #[test]
    fn the_jar_tells_the_hotbar_gate() {
        let td = tempfile::tempdir().unwrap();
        let v = td.path();
        jar(v, "1.21.1", Some(r#"{"world_version":3955,"stable":true}"#));
        jar(
            v,
            "25w10a",
            Some(r#"{"world_version":4300,"stable":false}"#),
        );
        jar(v, "1.12.2", None);
        let read = |mc| VersionFacts::read(v, mc).data;
        assert_eq!(read("1.21.1"), DataVersion::Version { n: 3955 });
        assert_eq!(read("25w10a"), DataVersion::NotRelease);
        assert_eq!(read("1.12.2"), DataVersion::NotRecorded);
        assert_eq!(read("1.20.4"), DataVersion::JarUnavailable);
    }

    #[test]
    fn hidden_servers_and_history_follow_the_version() {
        let f = |mc: &str| VersionFacts {
            mc_version: mc.into(),
            data: DataVersion::NotRecorded,
        };
        assert!(f("1.19").shows_hidden_servers());
        assert!(!f("1.18.2").shows_hidden_servers());
        assert!(f("24w10a").shows_hidden_servers());
        assert!(f("1.20.2").history_supported());
        assert!(!f("1.20.1").history_supported());
        assert!(!f("24w10a").history_supported());
    }
}

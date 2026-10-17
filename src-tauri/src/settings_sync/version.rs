//! What an instance's Minecraft version allows to sync (spec §B4–§B6).

use std::path::Path;

use serde::Serialize;

use super::options::catalog::{NEWEST_DV, OLDEST_DV};
use crate::datapacks::compat::{world_version_of_jar, JarWorldVersion};
use crate::instances::schema::LoaderKind;

/// The hotbars' gate: the DataVersion the instance's game saves with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DataVersion {
    Version {
        n: i32,
    },
    /// A jar that records no DataVersion (any version before 1.14).
    NotRecorded,
    /// The game files (the client jar, the version JSON) are not installed
    /// yet, or are damaged.
    JarUnavailable,
    /// A snapshot or pre-release, or a jar that does not say it is a release:
    /// its DataVersion would raise the gate for every release.
    NotRelease,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionFacts {
    pub mc_version: String,
    pub data: DataVersion,
    /// The installed version JSON says `"type": "release"`; `None` when it
    /// could not be read.
    pub release: Option<bool>,
    pub loader: LoaderKind,
}

/// 1.13.x jars record no DataVersion; these are the games' own `version`
/// lines (game-written options.txt, verified 2026-10-17).
const RELEASES_1_13: &[(&str, i32)] = &[("1.13", 1519), ("1.13.1", 1628), ("1.13.2", 1631)];

/// Why options.txt does not sync for a version (spec §C3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OptionsUnsupported {
    /// 1.12.2 and older.
    TooOld,
    /// A release newer than the catalog.
    TooNew,
    /// A snapshot or pre-release.
    NotRelease,
    /// The game files are not installed yet, or are damaged.
    NotInstalled,
}

impl VersionFacts {
    /// Reads the client jar: blocking, call it inside `spawn_blocking`.
    pub fn read(versions_dir: &Path, mc_version: &str, loader: LoaderKind) -> Self {
        let release = release_in(versions_dir, mc_version);
        let data = match world_version_of_jar(versions_dir, mc_version) {
            JarWorldVersion::JarUnavailable => DataVersion::JarUnavailable,
            JarWorldVersion::NotRecorded => DataVersion::NotRecorded,
            JarWorldVersion::Version(n) => match release {
                Some(true) => DataVersion::Version { n },
                Some(false) => DataVersion::NotRelease,
                // No version JSON to say what this game is.
                None => DataVersion::JarUnavailable,
            },
        };
        Self {
            mc_version: mc_version.to_string(),
            data,
            release,
            loader,
        }
    }

    /// The DataVersion options.txt syncs under, or why it does not: a
    /// release the catalog knows, read from the jar (1.14+) or, for 1.13.x,
    /// from the table of their own `version` lines.
    pub fn options_version(&self) -> Result<i32, OptionsUnsupported> {
        match self.data {
            DataVersion::Version { n } if n > NEWEST_DV => Err(OptionsUnsupported::TooNew),
            DataVersion::Version { n } if n < OLDEST_DV => Err(OptionsUnsupported::TooOld),
            DataVersion::Version { n } => Ok(n),
            DataVersion::NotRelease => Err(OptionsUnsupported::NotRelease),
            DataVersion::JarUnavailable => Err(OptionsUnsupported::NotInstalled),
            DataVersion::NotRecorded => match self.release {
                None => Err(OptionsUnsupported::NotInstalled),
                Some(false) => Err(OptionsUnsupported::NotRelease),
                Some(true) => RELEASES_1_13
                    .iter()
                    .find(|(mc, _)| *mc == self.mc_version)
                    .map(|(_, n)| *n)
                    .ok_or(OptionsUnsupported::TooOld),
            },
        }
    }

    /// Forge and NeoForge read a key modifier (`key.keyboard.a:SHIFT`).
    pub fn has_key_modifiers(&self) -> bool {
        matches!(self.loader, LoaderKind::Forge | LoaderKind::NeoForge)
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

/// Whether `mc_version` is a release, in the words of its installed version
/// JSON (Mojang's own file: `"type": "release"`). The jar's `stable` flag
/// is not it: the 1.14 and 1.14.1 jars say `false`. `None` when the file
/// is missing or does not parse: could not tell, which syncs nothing that
/// needs a release.
fn release_in(versions_dir: &Path, mc_version: &str) -> Option<bool> {
    let path = crate::versions::install::version_json_in(versions_dir, mc_version);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            crate::diag!("settings sync: could not read {} ({e})", path.display());
            return None;
        }
    };
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some(json.get("type")?.as_str()? == "release")
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

    fn version_json(versions: &Path, mc: &str, kind: &str) {
        let dir = versions.join(mc);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{mc}.json")),
            format!(r#"{{"id":"{mc}","type":"{kind}"}}"#),
        )
        .unwrap();
    }

    fn facts(mc: &str, data: DataVersion, release: Option<bool>) -> VersionFacts {
        VersionFacts {
            mc_version: mc.into(),
            data,
            release,
            loader: LoaderKind::Vanilla,
        }
    }

    /// The 1.14 and 1.14.1 jars say `"stable": false` (read 2026-10-17):
    /// whether a version is a release is the version JSON's word, not the
    /// jar's.
    #[test]
    fn a_release_is_what_its_version_json_says() {
        let td = tempfile::tempdir().unwrap();
        let v = td.path();
        jar(v, "1.14", Some(r#"{"world_version":1952,"stable":false}"#));
        version_json(v, "1.14", "release");
        jar(
            v,
            "25w10a",
            Some(r#"{"world_version":4300,"stable":false}"#),
        );
        version_json(v, "25w10a", "snapshot");
        jar(v, "1.21.1", Some(r#"{"world_version":3955,"stable":true}"#));
        let read = |mc| VersionFacts::read(v, mc, LoaderKind::Vanilla);
        assert_eq!(read("1.14").data, DataVersion::Version { n: 1952 });
        assert_eq!(read("1.14").release, Some(true));
        assert_eq!(read("25w10a").data, DataVersion::NotRelease);
        assert_eq!(
            read("1.21.1").data,
            DataVersion::JarUnavailable,
            "no version JSON: could not tell"
        );
        assert_eq!(read("1.21.1").release, None);
    }

    #[test]
    fn options_sync_from_1_13_to_the_newest_release_the_catalog_knows() {
        use DataVersion::{JarUnavailable, NotRecorded, Version};
        use OptionsUnsupported as O;
        assert_eq!(
            facts("1.13", NotRecorded, Some(true)).options_version(),
            Ok(1519)
        );
        assert_eq!(
            facts("1.13.1", NotRecorded, Some(true)).options_version(),
            Ok(1628)
        );
        assert_eq!(
            facts("1.13.2", NotRecorded, Some(true)).options_version(),
            Ok(1631)
        );
        assert_eq!(
            facts("1.12.2", NotRecorded, Some(true)).options_version(),
            Err(O::TooOld)
        );
        assert_eq!(
            facts("18w30a", NotRecorded, Some(false)).options_version(),
            Err(O::NotRelease)
        );
        assert_eq!(
            facts("1.13", NotRecorded, None).options_version(),
            Err(O::NotInstalled)
        );
        assert_eq!(
            facts("26.3", Version { n: 5023 }, Some(true)).options_version(),
            Ok(5023)
        );
        assert_eq!(
            facts("26.4", Version { n: 5100 }, Some(true)).options_version(),
            Err(O::TooNew)
        );
        assert_eq!(
            facts("26.4-pre1", DataVersion::NotRelease, Some(false)).options_version(),
            Err(O::NotRelease)
        );
        assert_eq!(
            facts("1.20.4", JarUnavailable, None).options_version(),
            Err(O::NotInstalled)
        );
    }

    #[test]
    fn only_forge_and_neoforge_read_a_key_modifier() {
        let with = |loader| VersionFacts {
            loader,
            ..facts("1.20.1", DataVersion::Version { n: 3465 }, Some(true))
        };
        assert!(with(LoaderKind::Forge).has_key_modifiers());
        assert!(with(LoaderKind::NeoForge).has_key_modifiers());
        assert!(!with(LoaderKind::Vanilla).has_key_modifiers());
        assert!(!with(LoaderKind::Fabric).has_key_modifiers());
        assert!(!with(LoaderKind::Quilt).has_key_modifiers());
    }

    #[test]
    fn the_jar_tells_the_hotbar_gate() {
        let td = tempfile::tempdir().unwrap();
        let v = td.path();
        jar(v, "1.21.1", Some(r#"{"world_version":3955,"stable":true}"#));
        version_json(v, "1.21.1", "release");
        jar(
            v,
            "25w10a",
            Some(r#"{"world_version":4300,"stable":false}"#),
        );
        version_json(v, "25w10a", "snapshot");
        jar(v, "1.12.2", None);
        let read = |mc| VersionFacts::read(v, mc, LoaderKind::Vanilla).data;
        assert_eq!(read("1.21.1"), DataVersion::Version { n: 3955 });
        assert_eq!(read("25w10a"), DataVersion::NotRelease);
        assert_eq!(read("1.12.2"), DataVersion::NotRecorded);
        assert_eq!(read("1.20.4"), DataVersion::JarUnavailable);
    }

    #[test]
    fn hidden_servers_and_history_follow_the_version() {
        let f = |mc: &str| facts(mc, DataVersion::NotRecorded, Some(true));
        assert!(f("1.19").shows_hidden_servers());
        assert!(!f("1.18.2").shows_hidden_servers());
        assert!(f("24w10a").shows_hidden_servers());
        assert!(f("1.20.2").history_supported());
        assert!(!f("1.20.1").history_supported());
        assert!(!f("24w10a").history_supported());
    }
}

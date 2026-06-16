//! Best-effort loader detection from an instance's `mods/` folder, used as a
//! fallback when a foreign launcher's metadata does not name the loader.
//! Pure + offline: reads each jar's descriptor via `read_jar_meta` and votes
//! a `LoaderKind`. A clear family always yields a concrete loader (never
//! `Vanilla`); the caller maps `None` to `Vanilla`.

use std::path::Path;

use crate::instances::schema::LoaderKind;
use crate::mods::local::{read_jar_meta, LoaderFamily};

/// Cap on how many jars we read before deciding — a majority vote needs only a
/// sample, and this can run during instance discovery.
const SNIFF_JAR_CAP: usize = 25;

/// Infer the mod loader from the jars in `mods_dir`. `None` when the folder has
/// no jar with a recognised descriptor, or when Fabric-family and Forge-family
/// votes tie (genuinely ambiguous). See module docs.
pub fn sniff_loader_from_mods(mods_dir: &Path) -> Option<LoaderKind> {
    let Ok(rd) = std::fs::read_dir(mods_dir) else {
        return None;
    };
    let (mut fabric, mut quilt, mut forge, mut neoforge) = (0u32, 0u32, 0u32, 0u32);
    let mut read = 0usize;
    for entry in rd.flatten() {
        if read >= SNIFF_JAR_CAP {
            break;
        }
        let path = entry.path();
        // Only enabled jars drive detection (a `.jar.disabled` is not loaded).
        if path.extension().map(|x| x != "jar").unwrap_or(true) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(meta) = read_jar_meta(&bytes) else {
            continue; // not a zip / unreadable — skip
        };
        read += 1;
        // Only single-family jars vote: a multi-loader jar (both descriptors)
        // runs on either loader and tells us nothing about the instance's.
        match meta.families.as_slice() {
            [LoaderFamily::Fabric] => {
                if meta.loader_label.as_deref() == Some("Quilt") {
                    quilt += 1;
                } else {
                    fabric += 1;
                }
            }
            [LoaderFamily::Forge] => {
                if meta.loader_label.as_deref() == Some("NeoForge") {
                    neoforge += 1;
                } else {
                    forge += 1;
                }
            }
            _ => {}
        }
    }
    decide(fabric, quilt, forge, neoforge)
}

/// Pick the loader from per-kind vote counts. Dominant family wins; within it
/// the more-voted kind wins, a tie falls back to the family base.
fn decide(fabric: u32, quilt: u32, forge: u32, neoforge: u32) -> Option<LoaderKind> {
    let fabric_fam = fabric + quilt;
    let forge_fam = forge + neoforge;
    if fabric_fam == 0 && forge_fam == 0 {
        return None;
    }
    if fabric_fam > forge_fam {
        Some(if quilt > fabric {
            LoaderKind::Quilt
        } else {
            LoaderKind::Fabric
        })
    } else if forge_fam > fabric_fam {
        Some(if neoforge > forge {
            LoaderKind::NeoForge
        } else {
            LoaderKind::Forge
        })
    } else {
        None // family tie — genuinely ambiguous
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    /// Build an in-memory `.jar` (zip) from (name, contents) entries.
    fn jar(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            for (name, body) in entries {
                w.start_file(*name, SimpleFileOptions::default()).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    /// A temp dir populated with the given (filename, bytes) jars.
    fn mods_dir_with(jars: &[(&str, Vec<u8>)]) -> tempfile::TempDir {
        let td = tempfile::TempDir::new().unwrap();
        for (name, bytes) in jars {
            std::fs::write(td.path().join(name), bytes).unwrap();
        }
        td
    }

    fn fabric_jar() -> Vec<u8> {
        jar(&[("fabric.mod.json", r#"{"id":"x","name":"X"}"#)])
    }
    fn forge_jar() -> Vec<u8> {
        jar(&[("META-INF/mods.toml", "modLoader=\"javafml\"")])
    }
    fn neoforge_jar() -> Vec<u8> {
        jar(&[("META-INF/neoforge.mods.toml", "modLoader=\"javafml\"")])
    }
    /// A realistic Quilt mod ships BOTH descriptors.
    fn quilt_jar() -> Vec<u8> {
        jar(&[
            ("quilt.mod.json", r#"{"quilt_loader":{"id":"x"}}"#),
            ("fabric.mod.json", r#"{"id":"x"}"#),
        ])
    }

    #[test]
    fn all_fabric_mods_detect_fabric() {
        let td = mods_dir_with(&[("a.jar", fabric_jar()), ("b.jar", fabric_jar())]);
        assert_eq!(sniff_loader_from_mods(td.path()), Some(LoaderKind::Fabric));
    }

    #[test]
    fn all_forge_mods_detect_forge() {
        let td = mods_dir_with(&[("a.jar", forge_jar())]);
        assert_eq!(sniff_loader_from_mods(td.path()), Some(LoaderKind::Forge));
    }

    #[test]
    fn neoforge_majority_detects_neoforge() {
        let td = mods_dir_with(&[
            ("a.jar", neoforge_jar()),
            ("b.jar", neoforge_jar()),
            ("c.jar", forge_jar()),
        ]);
        assert_eq!(
            sniff_loader_from_mods(td.path()),
            Some(LoaderKind::NeoForge)
        );
    }

    #[test]
    fn quilt_descriptor_detects_quilt() {
        let td = mods_dir_with(&[("a.jar", quilt_jar())]);
        assert_eq!(sniff_loader_from_mods(td.path()), Some(LoaderKind::Quilt));
    }

    #[test]
    fn forge_neoforge_tie_falls_back_to_forge() {
        let td = mods_dir_with(&[("a.jar", forge_jar()), ("b.jar", neoforge_jar())]);
        assert_eq!(sniff_loader_from_mods(td.path()), Some(LoaderKind::Forge));
    }

    #[test]
    fn no_descriptor_jars_yield_none() {
        let td = mods_dir_with(&[("lib.jar", jar(&[("data/x.txt", "nope")]))]);
        assert_eq!(sniff_loader_from_mods(td.path()), None);
    }

    #[test]
    fn empty_and_missing_dir_yield_none() {
        let td = tempfile::TempDir::new().unwrap();
        assert_eq!(sniff_loader_from_mods(td.path()), None);
        assert_eq!(sniff_loader_from_mods(&td.path().join("nope")), None);
    }

    #[test]
    fn mixed_families_tie_yields_none() {
        let td = mods_dir_with(&[("a.jar", fabric_jar()), ("b.jar", forge_jar())]);
        assert_eq!(sniff_loader_from_mods(td.path()), None);
    }

    #[test]
    fn disabled_jars_do_not_vote() {
        // Only enabled `.jar` files drive detection; a `.jar.disabled` is skipped.
        let td = mods_dir_with(&[("a.jar.disabled", fabric_jar())]);
        assert_eq!(sniff_loader_from_mods(td.path()), None);
    }

    #[test]
    fn decide_prefers_dominant_family() {
        assert_eq!(decide(3, 0, 1, 0), Some(LoaderKind::Fabric));
        assert_eq!(decide(0, 0, 1, 2), Some(LoaderKind::NeoForge));
        assert_eq!(decide(1, 0, 1, 0), None); // family tie
        assert_eq!(decide(0, 0, 0, 0), None); // no votes
    }
}

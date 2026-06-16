use std::path::Path;

use crate::instances::import::model::ForeignInstance;
use crate::instances::import::readers::{
    raw_minecraft::RawMinecraftReader, structured_readers, LauncherReader,
};

/// Scan one `.minecraft`-shaped root: structured profile readers first, then
/// the raw reader for a bare `.minecraft` (no `launcher_profiles.json`).
/// Deduped by `minecraft_dir`; a structured entry beats a raw one.
pub fn scan_minecraft_root(root: &Path) -> Vec<ForeignInstance> {
    let mut out: Vec<ForeignInstance> = Vec::new();
    for reader in structured_readers() {
        for fi in reader.expand_root(root) {
            if !out.iter().any(|e| e.minecraft_dir == fi.minecraft_dir) {
                out.push(fi);
            }
        }
    }
    // Raw fallback: a `.minecraft` not already covered by a structured reader,
    // with real content. ProfileReader already handles the
    // launcher_profiles.json case.
    if RawMinecraftReader.detect(root) {
        if let Ok(fi) = RawMinecraftReader.read(root) {
            let has_content = !fi.content.is_empty();
            let covered = out.iter().any(|e| e.minecraft_dir == fi.minecraft_dir);
            if has_content && !covered {
                out.push(fi);
            }
        }
    }
    out
}

/// Scan all structured readers' default roots and return every readable
/// instance. `.minecraft`-shaped roots go through `scan_minecraft_root` so a
/// bare vanilla install (no `launcher_profiles.json`) is found via the raw
/// fallback, and every entry is globally deduped by `minecraft_dir`.
/// Best-effort: missing roots and unreadable instances are skipped silently.
pub fn discover_all() -> Vec<ForeignInstance> {
    let mut out: Vec<ForeignInstance> = Vec::new();
    for reader in structured_readers() {
        for root in reader.default_roots() {
            // `.minecraft`-shaped roots go through the raw-aware sweep so a
            // bare vanilla install is found and deduped; other roots use the
            // reader's own expand_root.
            let is_dot_mc = root.ends_with(".minecraft") || root.ends_with("minecraft");
            let found = if is_dot_mc {
                scan_minecraft_root(&root)
            } else {
                reader.expand_root(&root)
            };
            for fi in found {
                if !out.iter().any(|e| e.minecraft_dir == fi.minecraft_dir) {
                    out.push(fi);
                }
            }
        }
    }
    out
}

/// Scan one root with every structured reader (used by tests and by a
/// user-pointed launcher root). Public for `discover_all` + tests.
pub fn scan_root(root: &Path) -> Vec<ForeignInstance> {
    let mut out = Vec::new();
    for reader in structured_readers() {
        out.extend(reader.expand_root(root));
    }
    out
}

/// Detect a single user-picked folder. Tries structured readers first,
/// then the generic `.minecraft` reader. `None` if nothing matches.
pub fn detect_folder(dir: &Path) -> Option<ForeignInstance> {
    for reader in structured_readers() {
        if reader.detect(dir) {
            if let Ok(fi) = reader.read(dir) {
                return Some(fi);
            }
        }
    }
    if RawMinecraftReader.detect(dir) {
        return RawMinecraftReader.read(dir).ok();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixtures() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[test]
    fn scan_root_lists_prism_instances() {
        // The fixtures dir contains prism_* instance folders.
        let found = scan_root(&fixtures());
        let names: Vec<_> = found.iter().map(|f| f.name.clone()).collect();
        assert!(
            names.iter().any(|n| n == "Fabulously Optimized"),
            "got: {names:?}"
        );
    }

    #[test]
    fn detect_folder_matches_prism_then_raw() {
        // A prism instance is detected as Prism, not raw.
        let r = detect_folder(&fixtures().join("prism_fabric")).unwrap();
        assert_eq!(r.source, crate::instances::schema::ForeignLauncher::Prism);
        // A bare .minecraft is detected as RawMinecraft.
        let r2 = detect_folder(&fixtures().join("raw_minecraft")).unwrap();
        assert_eq!(
            r2.source,
            crate::instances::schema::ForeignLauncher::RawMinecraft
        );
    }

    #[test]
    fn scan_minecraft_root_falls_back_to_raw_for_bare_dir() {
        // A bare `.minecraft` with content but no structured markers surfaces
        // exactly once, as a RawMinecraft entry.
        let found = scan_minecraft_root(&fixtures().join("raw_minecraft"));
        assert_eq!(found.len(), 1, "single raw entry, got: {found:?}");
        assert_eq!(
            found[0].source,
            crate::instances::schema::ForeignLauncher::RawMinecraft
        );
    }

    #[test]
    fn detect_folder_rejects_unrelated_dir() {
        assert!(detect_folder(Path::new(env!("CARGO_MANIFEST_DIR"))).is_none());
    }

    #[test]
    fn discover_dedups_same_dir_across_readers() {
        // A profile-style shared .minecraft is also raw-shaped; it must
        // appear once (the structured entry wins).
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join(".minecraft");
        std::fs::create_dir_all(mc.join("saves/W")).unwrap();
        std::fs::write(mc.join("saves/W/level.dat"), b"x").unwrap();
        std::fs::write(mc.join("launcher_profiles.json"), "{}").unwrap();

        let found = scan_minecraft_root(&mc);
        let at_mc: Vec<_> = found.iter().filter(|f| f.minecraft_dir == mc).collect();
        assert_eq!(at_mc.len(), 1, "deduped to one, got: {at_mc:?}");
        assert_ne!(
            at_mc[0].source,
            crate::instances::schema::ForeignLauncher::RawMinecraft,
            "structured (profile) entry should win over raw"
        );
    }

    #[test]
    fn scan_root_expands_profile_minecraft() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join(".minecraft");
        let game = mc.join("versions/test");
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::write(game.join("mods/a.jar"), b"x").unwrap();
        std::fs::write(game.join("test.json"), r#"{"id":"1.20.1"}"#).unwrap();

        let found = scan_root(&mc);
        assert!(
            found.iter().any(|f| f.name == "test"
                && f.source == crate::instances::schema::ForeignLauncher::MojangLauncher),
            "got: {:?}",
            found.iter().map(|f| &f.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn detect_folder_matches_profile_versions_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join(".minecraft/versions/test");
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::write(game.join("mods/a.jar"), b"x").unwrap();
        std::fs::write(game.join("test.json"), r#"{"id":"1.20.1"}"#).unwrap();

        let fi = detect_folder(&game).unwrap();
        assert_eq!(
            fi.source,
            crate::instances::schema::ForeignLauncher::MojangLauncher
        );
    }
}

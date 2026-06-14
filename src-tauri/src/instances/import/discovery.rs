use std::path::Path;

use crate::instances::import::model::ForeignInstance;
use crate::instances::import::readers::{
    raw_minecraft::RawMinecraftReader, structured_readers, LauncherReader,
};

/// Scan all structured readers' default roots and return every readable
/// instance. Best-effort: missing roots and unreadable instances are
/// skipped silently (one bad instance never fails the whole scan).
pub fn discover_all() -> Vec<ForeignInstance> {
    let mut out = Vec::new();
    for reader in structured_readers() {
        for root in reader.default_roots() {
            out.extend(reader.expand_root(&root));
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
    fn detect_folder_rejects_unrelated_dir() {
        assert!(detect_folder(Path::new(env!("CARGO_MANIFEST_DIR"))).is_none());
    }
}

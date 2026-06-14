use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::instances::import::model::{scan_content, ForeignInstance};
use crate::instances::import::readers::LauncherReader;
use crate::instances::schema::{ForeignLauncher, LoaderKind};

/// Fallback reader for a bare `.minecraft`-shaped folder. Carries no
/// version/loader metadata — the UI requires the user to supply those.
pub struct RawMinecraftReader;

impl RawMinecraftReader {
    /// A dir "looks like" a `.minecraft` if it has any of the canonical
    /// content subdirs/files.
    fn looks_like_minecraft(dir: &Path) -> bool {
        ["mods", "saves", "config", "resourcepacks"]
            .iter()
            .any(|d| dir.join(d).is_dir())
            || dir.join("options.txt").is_file()
    }
}

impl LauncherReader for RawMinecraftReader {
    fn launcher(&self) -> ForeignLauncher {
        ForeignLauncher::RawMinecraft
    }
    fn default_roots(&self) -> Vec<PathBuf> {
        // No auto-discovery: a bare .minecraft is manual-pick only.
        vec![]
    }
    fn detect(&self, dir: &Path) -> bool {
        Self::looks_like_minecraft(dir)
    }
    fn read(&self, dir: &Path) -> Result<ForeignInstance> {
        let name = dir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| s != ".minecraft")
            .or_else(|| {
                dir.parent()
                    .and_then(|p| p.file_name())
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "Imported".to_string());
        Ok(ForeignInstance {
            source: ForeignLauncher::RawMinecraft,
            name,
            root: dir.to_path_buf(),
            minecraft_dir: dir.to_path_buf(),
            mc_version: String::new(),   // user supplies
            loader: LoaderKind::Vanilla, // user may override
            loader_version: None,
            max_heap_mb: None,
            extra_jvm_args: None,
            content: scan_content(dir),
            known_mods: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::import::model::ContentCategory;
    use crate::instances::schema::LoaderKind;
    use std::path::Path;

    fn raw() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw_minecraft")
    }

    #[test]
    fn detects_a_minecraft_folder() {
        assert!(RawMinecraftReader.detect(&raw()));
    }

    #[test]
    fn rejects_a_dir_without_minecraft_markers() {
        assert!(!RawMinecraftReader.detect(Path::new(env!("CARGO_MANIFEST_DIR"))));
    }

    #[test]
    fn reads_content_with_unknown_version_and_loader() {
        let fi = RawMinecraftReader.read(&raw()).unwrap();
        // Version/loader are user-supplied later — reader leaves them blank.
        assert_eq!(fi.mc_version, "");
        assert_eq!(fi.loader, LoaderKind::Vanilla);
        assert_eq!(fi.loader_version, None);
        assert!(fi
            .content
            .iter()
            .any(|c| c.category == ContentCategory::Mods));
        assert!(fi
            .content
            .iter()
            .any(|c| c.category == ContentCategory::OptionsTxt));
    }
}

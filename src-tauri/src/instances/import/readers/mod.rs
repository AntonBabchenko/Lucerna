//! Per-launcher readers. Each parses one foreign format into a
//! `ForeignInstance`. The registry lists every reader for discovery and
//! manual-folder detection.

pub mod prism;
pub mod profile;
pub mod raw_minecraft;

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::instances::import::model::ForeignInstance;
use crate::instances::schema::ForeignLauncher;

pub trait LauncherReader: Send + Sync {
    fn launcher(&self) -> ForeignLauncher;
    /// Standard OS locations where this launcher keeps its instances.
    fn default_roots(&self) -> Vec<PathBuf>;
    /// True iff `dir` is one instance of this launcher.
    fn detect(&self, dir: &Path) -> bool;
    /// Parse `dir` into a normalized `ForeignInstance`.
    fn read(&self, dir: &Path) -> Result<ForeignInstance>;
    /// Expand one launcher root into every instance under it. Default:
    /// each direct child of `root` is a candidate instance (the Prism /
    /// MultiMC / ATLauncher model). Readers whose layout is one root
    /// holding several game directories (the profile model) override this.
    fn expand_root(&self, root: &Path) -> Vec<ForeignInstance> {
        let Ok(rd) = std::fs::read_dir(root) else {
            return vec![];
        };
        rd.flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && self.detect(p))
            .filter_map(|p| self.read(&p).ok())
            .collect()
    }
}

/// Every structured reader (the generic `.minecraft` reader is handled
/// separately — it has no auto-discovery and matches any `.minecraft`).
pub fn structured_readers() -> Vec<Box<dyn LauncherReader>> {
    vec![
        Box::new(prism::PrismReader),
        Box::new(profile::ProfileReader),
    ]
}

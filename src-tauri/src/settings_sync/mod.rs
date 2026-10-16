//! Settings sync (spec 2026-10-15 Part B): an instance can share its server
//! list, command history and hotbars with every other instance that opts in.
//!
//! One shared copy per kind lives in `<data_root>/shared/settings/`; each
//! member instance keeps a sidecar (`lucerna/settings-sync.json`) and, per
//! kind, a baseline of what its file held when Lucerna last touched it. A
//! *sync point* (before a launch, after the game exits, on join) captures what
//! changed in this instance since that baseline into the shared copy and
//! applies the shared copy back. Everything here takes `&Path` roots: no
//! `AppHandle`, so it is tested directly.

pub(crate) mod baseline;
pub(crate) mod fsio;
pub(crate) mod history;
pub(crate) mod hotbars;
pub(crate) mod servers;
pub mod sidecar;
pub mod store;
pub mod version;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One thing that syncs. Part C adds `Options`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Servers,
    History,
    Hotbars,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Servers, Kind::History, Kind::Hotbars];

    /// The file the game keeps for this kind, in `.minecraft/` and in the
    /// shared store alike.
    pub fn file(self) -> &'static str {
        match self {
            Kind::Servers => "servers.dat",
            Kind::History => "command_history.txt",
            Kind::Hotbars => "hotbar.nbt",
        }
    }

    fn stem(self) -> &'static str {
        match self {
            Kind::Servers => "servers",
            Kind::History => "history",
            Kind::Hotbars => "hotbars",
        }
    }
}

/// `<instance>/lucerna/settings-sync.json`.
pub fn sidecar_path(instance_root: &Path) -> PathBuf {
    instance_root.join("lucerna").join("settings-sync.json")
}

/// `<instance>/lucerna/settings-sync/`: baselines, pending files, the marker.
pub fn state_dir(instance_root: &Path) -> PathBuf {
    instance_root.join("lucerna").join("settings-sync")
}

/// `<instance>/.minecraft/<file>`.
pub fn game_file(instance_root: &Path, kind: Kind) -> PathBuf {
    instance_root.join(".minecraft").join(kind.file())
}

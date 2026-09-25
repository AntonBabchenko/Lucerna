//! Pack format versions and what a data pack's `pack.mcmeta` declares.
//!
//! Facts only, never a verdict: whether a pack loads, and how the game labels
//! it, depends on the instance's Minecraft version, which can change without a
//! reinstall — `datapacks::verdict` combines the two at listing time.
//!
//! Internal, not IPC: the UI only ever receives the verdict (`PackCompat`).

/// A pack format version as the game compares it: major first, then minor
/// (26.2's `PackFormat.compareTo`). Before data format 82 (1.21.9) there is
/// no minor and every version reads as `N.0`. The derived `Ord` compares the
/// fields in declaration order, which IS that rule — keep `major` first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormatVersion {
    pub major: u32,
    pub minor: u32,
}

impl FormatVersion {
    #[must_use]
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

/// Which half of a client jar's `version.json` `pack_version` to read: the
/// resource-pack format (`l10n::pack_format`) or the data-pack format
/// (`datapacks::compat::game_data_format`). The two diverge from 1.18.2 on,
/// so a reader must never pick one for the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackSide {
    Resource,
    Data,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_versions_order_by_major_then_minor() {
        assert!(FormatVersion::new(94, 1) > FormatVersion::new(94, 0));
        assert!(FormatVersion::new(95, 0) > FormatVersion::new(94, u32::MAX));
        assert!(FormatVersion::new(48, 0) < FormatVersion::new(94, 1));
    }
}

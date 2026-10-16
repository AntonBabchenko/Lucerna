//! One-time copy of mod data from another instance (spec 2026-10-15 Part A):
//! what to copy (`catalog`), how a root is walked (`walk`), what the dialog
//! shows (`preview`), and the copy itself (`engine`, committed through
//! `mods::txn`). A clone copies the same catalog, whole trees
//! ([`copy_for_clone`]).

pub mod catalog;
pub mod engine;
pub mod preview;
pub mod walk;

use std::path::Path;

pub use catalog::TransferItemId;
pub use engine::{OnConflict, TransferOutcome, TransferPhase, TransferProgress, TransferSelection};
pub use preview::{ItemPreview, OtherPreview, Refusal, RootPreview, TransferPreview};

use crate::error::Result;

/// Mod data a clone would copy, for the clone dialog's row (spec §A6).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, specta::Type)]
pub struct ModDataSummary {
    pub files: u32,
    pub bytes: f64,
    /// False when a root cannot be read or links to a missing folder: a clone
    /// is all-or-nothing, so the row is off.
    pub selectable: bool,
}

/// What [`copy_for_clone`] would take from `src_mc`.
pub fn clone_summary(src_mc: &Path) -> ModDataSummary {
    let _ = src_mc;
    todo!("red-first stub")
}

/// Copy every catalog item the source has into a fresh clone's game dir, whole
/// trees. Any failure is fatal: the clone rolls its whole folder back.
pub fn copy_for_clone(
    src_mc: &Path,
    dst_mc: &Path,
    progress: &mut dyn FnMut(u32, u32),
) -> Result<()> {
    let _ = (src_mc, dst_mc, progress);
    todo!("red-first stub")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::transfer::walk::join_rel;

    fn write(base: &Path, rel: &str) {
        let p = join_rel(base, rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn the_clone_summary_counts_whole_map_trees() {
        let td = tempfile::tempdir().unwrap();
        write(td.path(), "xaero/minimap/New World/sp.txt");
        write(td.path(), "xaero/minimap/Multiplayer_a/mp.txt");
        write(td.path(), "schematics/a.nbt");
        let s = clone_summary(td.path());
        assert_eq!((s.files, s.selectable), (3, true));
    }

    #[test]
    fn an_empty_game_dir_has_nothing_for_a_clone() {
        let td = tempfile::tempdir().unwrap();
        assert_eq!(
            clone_summary(td.path()),
            ModDataSummary {
                files: 0,
                bytes: 0.0,
                selectable: true
            }
        );
    }
}

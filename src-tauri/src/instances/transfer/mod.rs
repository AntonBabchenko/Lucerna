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

use crate::content_root::{content_at, Content, Expect};
use crate::error::{Error, Result, TransferRole};
use crate::instances::maintenance::MaintenanceGuard;

/// Both instances' maintenance claims for one copy: the target's, then the
/// source's. Neither can then be launched or written until the guards drop.
/// A refusal names the busy instance and its side (`InstanceBusy` names
/// none); a refused source claim drops the target's before the error leaves.
pub fn claim_both(target: (&str, &str), source: (&str, &str)) -> Result<BothClaims> {
    let busy = |name: &str, role: TransferRole| {
        let name = name.to_string();
        move |e: Error| match e {
            Error::InstanceBusy => Error::TransferInstanceBusy {
                instance_name: name,
                role,
            },
            other => other,
        }
    };
    let target_claim = crate::instances::maintenance::claim_write(target.0)
        .map_err(busy(target.1, TransferRole::Target))?;
    let source_claim = crate::instances::maintenance::claim_write(source.0)
        .map_err(busy(source.1, TransferRole::Source))?;
    Ok(BothClaims {
        _target: target_claim,
        _source: source_claim,
    })
}

/// The two claims [`claim_both`] took; dropping it releases both. One value,
/// so the command holds and drops it by one name (the maintenance-gate guard
/// checks that release against the body's last `.await`).
pub struct BothClaims {
    _target: MaintenanceGuard,
    _source: MaintenanceGuard,
}

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
    let mut out = ModDataSummary {
        files: 0,
        bytes: 0.0,
        selectable: true,
    };
    for id in catalog::ALL {
        for root in id.clone_roots() {
            let path = walk::join_rel(src_mc, root.rel);
            // The row only says "unavailable"; the log names the folder and why.
            match content_at(&path, Expect::Dir) {
                Ok(Content::Nothing) => {}
                Ok(Content::Local | Content::Linked { .. }) => {
                    match walk::walk_root(src_mc, root.rel, root.scope) {
                        Ok(w) => {
                            out.files += w.files.len() as u32;
                            out.bytes += w.bytes() as f64;
                        }
                        Err(e) => {
                            crate::diag!("transfer: clone cannot read {} ({e})", path.display());
                            out.selectable = false;
                        }
                    }
                }
                Ok(Content::BrokenLink { target }) => {
                    crate::diag!(
                        "transfer: clone cannot copy {}: it links to {}, which is missing",
                        path.display(),
                        target.display()
                    );
                    out.selectable = false;
                }
                Err(e) => {
                    crate::diag!("transfer: clone cannot read {} ({e})", path.display());
                    out.selectable = false;
                }
            }
        }
    }
    out
}

/// `<instance>/lucerna/replaced/<name>`: where a copy kept the files it
/// replaced, for the startup notice's "Open folder". `name` comes from the
/// webview, so it must be one plain folder name; the folder must already be
/// a folder: opening never creates it, and never follows a link out of the
/// instance.
pub fn kept_folder(instance_root: &Path, name: &str) -> Result<std::path::PathBuf> {
    let dir = instance_root.join("lucerna").join("replaced").join(name);
    if !crate::pathsafe::is_safe_filename(name) {
        return Err(Error::ModsInstancePath {
            path: name.to_string(),
            details: "not a folder name".into(),
        });
    }
    match std::fs::symlink_metadata(&dir) {
        Ok(m) if m.is_dir() => Ok(dir),
        Ok(_) => Err(Error::ModsInstancePath {
            path: dir.display().to_string(),
            details: "not a folder".into(),
        }),
        Err(e) => Err(Error::io(dir.display().to_string(), e)),
    }
}

/// Copy every catalog item the source has into a fresh clone's game dir, whole
/// trees. Any failure is fatal: the clone rolls its whole folder back.
pub fn copy_for_clone(
    src_mc: &Path,
    dst_mc: &Path,
    progress: &mut dyn FnMut(u32, u32),
) -> Result<()> {
    let mut files = Vec::new();
    for id in catalog::ALL {
        for root in id.clone_roots() {
            let path = walk::join_rel(src_mc, root.rel);
            let present = crate::content_root::has_content(&path, Expect::Dir)
                .map_err(|e| Error::io(path.display().to_string(), e))?;
            if present {
                let w = walk::walk_root(src_mc, root.rel, root.scope)
                    .map_err(|e| Error::io(path.display().to_string(), e))?;
                files.extend(w.files);
            }
        }
    }
    let total = files.len() as u32;
    for (i, f) in files.iter().enumerate() {
        let from = walk::join_rel(src_mc, &f.rel);
        let to = walk::join_rel(dst_mc, &f.rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io(parent.display().to_string(), e))?;
        }
        std::fs::copy(&from, &to).map_err(|e| Error::io(to.display().to_string(), e))?;
        progress(i as u32 + 1, total);
    }
    Ok(())
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
    fn a_busy_source_is_named_and_the_target_claim_is_released() {
        let held = crate::instances::maintenance::claim_write("transfer-test-src-1").unwrap();
        let err = claim_both(
            ("transfer-test-dst-1", "Dst"),
            ("transfer-test-src-1", "Src"),
        )
        .err()
        .expect("the source is held");
        assert_eq!(
            err,
            Error::TransferInstanceBusy {
                instance_name: "Src".into(),
                role: TransferRole::Source
            }
        );
        drop(held);
        let again = crate::instances::maintenance::claim_write("transfer-test-dst-1");
        assert!(again.is_ok(), "the target claim was released");
    }

    #[test]
    fn a_busy_target_is_named() {
        let held = crate::instances::maintenance::claim_write("transfer-test-dst-2").unwrap();
        let err = claim_both(
            ("transfer-test-dst-2", "Dst"),
            ("transfer-test-src-2", "Src"),
        )
        .err()
        .expect("the target is held");
        assert_eq!(
            err,
            Error::TransferInstanceBusy {
                instance_name: "Dst".into(),
                role: TransferRole::Target
            }
        );
        drop(held);
    }

    /// "Open folder" on the kept-files notice: one existing folder directly in
    /// `lucerna/replaced`, never a path, never created, never a link out.
    #[test]
    fn the_kept_folder_is_one_existing_folder_under_replaced() {
        let td = tempfile::tempdir().unwrap();
        let replaced = td.path().join("lucerna").join("replaced");
        let name = "Replaced by Lucerna – Pack – 2026-10-16 05-19-30 (2)";
        std::fs::create_dir_all(replaced.join(name)).unwrap();
        assert_eq!(kept_folder(td.path(), name).unwrap(), replaced.join(name));
        for bad in ["", ".", "..", "a/b", "a\\b", "C:x", "/abs"] {
            assert!(kept_folder(td.path(), bad).is_err(), "{bad:?}");
        }
        assert!(kept_folder(td.path(), "missing").is_err());
        assert!(
            !replaced.join("missing").exists(),
            "opening never creates the folder"
        );
        std::fs::write(replaced.join("file"), b"x").unwrap();
        assert!(kept_folder(td.path(), "file").is_err());
        let outside = td.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        crate::content_root::test_links::link_dir(&outside, &replaced.join("link"));
        assert!(
            kept_folder(td.path(), "link").is_err(),
            "a link is not followed"
        );
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

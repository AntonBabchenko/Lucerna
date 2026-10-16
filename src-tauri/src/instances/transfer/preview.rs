//! What the transfer dialog shows (spec §A3): for each catalog item and each
//! "Other folder" the source has, where its roots are on both sides, what would
//! be copied, how many target files share a name with it, and whether anything
//! makes the copy unsafe (§A5). Read-only on both sides.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use super::catalog::{self, Scope, TransferItemId};
use super::walk::{join_rel, walk_root, Walked};
use crate::content_root::{content_at, display, inspect, Content, Expect, Resolved, RootKind};
use crate::instances::import::model::ContentLocation;

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct TransferPreview {
    pub items: Vec<ItemPreview>,
    pub other: Vec<OtherPreview>,
    /// Free space on the target's volume, MB; `None` when it cannot be read.
    pub target_free_mb: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ItemPreview {
    pub id: TransferItemId,
    pub roots: Vec<RootPreview>,
    pub files: u32,
    pub bytes: f64,
    pub conflicts: u32,
    pub skipped: u32,
    pub refusal: Option<Refusal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct OtherPreview {
    pub name: String,
    pub root: RootPreview,
    pub files: u32,
    pub bytes: f64,
    pub conflicts: u32,
    pub skipped: u32,
    pub refusal: Option<Refusal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct RootPreview {
    pub rel: String,
    pub source: ContentLocation,
    /// `None` when the target has no such folder yet.
    pub target: Option<ContentLocation>,
}

/// Why an item cannot be copied. Each names the `/`-separated path it is about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Refusal {
    /// A source or target folder is a link whose target is missing.
    BrokenLink { rel: String },
    /// A source or target path could not be read.
    Unreadable { rel: String, reason: String },
    /// A target file where the copy needs a folder, or a folder where it has a file.
    PrefixClash { rel: String },
    /// A link inside the target where the copy would write.
    InnerLink { rel: String },
    /// Both instances use the same folder.
    Overlap { rel: String },
}

/// One root, both sides.
pub(crate) struct RootScan {
    pub preview: RootPreview,
    pub walked: Walked,
    pub conflicts: u32,
    pub refusal: Option<Refusal>,
}

pub fn preview(
    source_mc: &Path,
    target_mc: &Path,
    target_free_mb: Option<u64>,
) -> std::io::Result<TransferPreview> {
    let mut items = Vec::new();
    for id in catalog::ALL {
        let scans: Vec<RootScan> = id
            .transfer_roots()
            .iter()
            .filter_map(|r| scan_root(source_mc, target_mc, r.rel, r.scope))
            .collect();
        if scans.is_empty() {
            continue;
        }
        let mut item = ItemPreview {
            id,
            roots: Vec::new(),
            files: 0,
            bytes: 0.0,
            conflicts: 0,
            skipped: 0,
            refusal: None,
        };
        for s in scans {
            item.files += s.walked.files.len() as u32;
            item.bytes += s.walked.bytes() as f64;
            item.conflicts += s.conflicts;
            item.skipped += s.walked.skipped;
            item.refusal = item.refusal.or(s.refusal);
            item.roots.push(s.preview);
        }
        items.push(item);
    }
    let mut other = Vec::new();
    for name in other_folder_names(source_mc)? {
        if let Some(s) = scan_root(source_mc, target_mc, &name, Scope::WholeTree) {
            other.push(OtherPreview {
                name,
                files: s.walked.files.len() as u32,
                bytes: s.walked.bytes() as f64,
                conflicts: s.conflicts,
                skipped: s.walked.skipped,
                refusal: s.refusal,
                root: s.preview,
            });
        }
    }
    Ok(TransferPreview {
        items,
        other,
        target_free_mb: target_free_mb.map(|mb| mb as f64),
    })
}

/// The source game dir's top-level folders "Other folders" offers: real
/// folders and links to folders. Listing the game dir itself failing is an
/// error — never "nothing to offer".
pub(crate) fn other_folder_names(source_mc: &Path) -> std::io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(source_mc)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue; // not offered: a name the transaction cannot record
        };
        if !catalog::is_offered_other(&name) {
            continue;
        }
        let folder = matches!(
            inspect(&entry.path())?,
            RootKind::Dir
                | RootKind::Linked {
                    resolves_to: Resolved::Dir,
                    ..
                }
        );
        if folder {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// One root, both sides. `None` when the source has nothing at `rel`.
pub(crate) fn scan_root(
    source_mc: &Path,
    target_mc: &Path,
    rel: &str,
    scope: Scope,
) -> Option<RootScan> {
    let source_path = join_rel(source_mc, rel);
    let (source, source_refusal) = match content_at(&source_path, Expect::Dir) {
        Ok(Content::Nothing) => return None,
        Ok(Content::Local) => (ContentLocation::Local, None),
        Ok(Content::Linked { target }) => (
            ContentLocation::Linked {
                target: display(&target),
            },
            None,
        ),
        Ok(Content::BrokenLink { target }) => (
            ContentLocation::BrokenLink {
                target: display(&target),
            },
            Some(Refusal::BrokenLink { rel: rel.into() }),
        ),
        Err(e) => (
            ContentLocation::Unreadable {
                reason: e.to_string(),
            },
            Some(Refusal::Unreadable {
                rel: rel.into(),
                reason: e.to_string(),
            }),
        ),
    };
    let target_state = target_root(target_mc, rel);
    let target = match &target_state {
        Ok(TargetRoot::Present(loc)) => Some(loc.clone()),
        Ok(TargetRoot::Absent) | Err(_) => None,
    };
    let mut scan = RootScan {
        preview: RootPreview {
            rel: rel.into(),
            source,
            target,
        },
        walked: Walked::default(),
        conflicts: 0,
        refusal: source_refusal,
    };
    if scan.refusal.is_some() {
        return Some(scan);
    }
    if let Err(r) = target_state {
        scan.refusal = Some(r);
        return Some(scan);
    }
    match walk_root(source_mc, rel, scope) {
        Ok(w) => scan.walked = w,
        Err(e) => {
            scan.refusal = Some(Refusal::Unreadable {
                rel: rel.into(),
                reason: e.to_string(),
            });
            return Some(scan);
        }
    }
    if let Err(r) = overlap(&source_path, target_mc, rel) {
        scan.refusal = Some(r);
        return Some(scan);
    }
    let mut dirs: HashMap<PathBuf, DirState> = HashMap::new();
    for f in &scan.walked.files {
        match target_file(target_mc, rel, &f.rel, &mut dirs) {
            Ok(FileState::Free) => {}
            Ok(FileState::Taken) => scan.conflicts += 1,
            Err(r) => {
                scan.refusal = Some(r);
                break;
            }
        }
    }
    Some(scan)
}

enum TargetRoot {
    Absent,
    Present(ContentLocation),
}

/// The target's folder at `rel`, segment by segment from the game dir: absent
/// from some segment on, or present. A link is allowed only as the top-level
/// folder (#507: a link at a content root is content); deeper, the copy would
/// write out of the instance through it.
fn target_root(target_mc: &Path, rel: &str) -> Result<TargetRoot, Refusal> {
    let mut path = target_mc.to_path_buf();
    let mut location = ContentLocation::Local;
    let mut so_far = String::new();
    for (i, seg) in rel.split('/').filter(|s| !s.is_empty()).enumerate() {
        path.push(seg);
        so_far = if so_far.is_empty() {
            seg.to_string()
        } else {
            format!("{so_far}/{seg}")
        };
        match inspect(&path) {
            Ok(RootKind::Absent) => return Ok(TargetRoot::Absent),
            Ok(RootKind::Dir) => {}
            Ok(RootKind::Linked {
                target,
                resolves_to: Resolved::Dir,
            }) if i == 0 => {
                location = ContentLocation::Linked {
                    target: display(&target),
                };
            }
            Ok(RootKind::Linked {
                resolves_to: Resolved::Missing,
                ..
            }) => return Err(Refusal::BrokenLink { rel: so_far }),
            Ok(RootKind::Linked {
                resolves_to: Resolved::Dir,
                ..
            }) => return Err(Refusal::InnerLink { rel: so_far }),
            Ok(RootKind::File | RootKind::Other | RootKind::Linked { .. }) => {
                return Err(Refusal::PrefixClash { rel: so_far })
            }
            Err(e) => {
                return Err(Refusal::Unreadable {
                    rel: so_far,
                    reason: e.to_string(),
                })
            }
        }
    }
    Ok(TargetRoot::Present(location))
}

#[derive(Clone, Copy)]
enum DirState {
    Absent,
    Dir,
}

enum FileState {
    Free,
    Taken,
}

/// Where a copied file would land: free, taken by a file (a conflict), or
/// unsafe. Folders below the root are classified once (`dirs`).
fn target_file(
    target_mc: &Path,
    root_rel: &str,
    file_rel: &str,
    dirs: &mut HashMap<PathBuf, DirState>,
) -> Result<FileState, Refusal> {
    let below = file_rel
        .strip_prefix(root_rel)
        .unwrap_or(file_rel)
        .trim_start_matches('/');
    let segs: Vec<&str> = below.split('/').filter(|s| !s.is_empty()).collect();
    let mut path = join_rel(target_mc, root_rel);
    let mut rel = root_rel.to_string();
    for (i, seg) in segs.iter().enumerate() {
        path.push(seg);
        rel = format!("{rel}/{seg}");
        if i + 1 < segs.len() {
            let state = match dirs.get(&path) {
                Some(s) => *s,
                None => {
                    let s = match inspect(&path) {
                        Ok(RootKind::Absent) => DirState::Absent,
                        Ok(RootKind::Dir) => DirState::Dir,
                        Ok(RootKind::Linked { .. }) => return Err(Refusal::InnerLink { rel }),
                        Ok(RootKind::File | RootKind::Other) => {
                            return Err(Refusal::PrefixClash { rel })
                        }
                        Err(e) => {
                            return Err(Refusal::Unreadable {
                                rel,
                                reason: e.to_string(),
                            })
                        }
                    };
                    dirs.insert(path.clone(), s);
                    s
                }
            };
            if let DirState::Absent = state {
                return Ok(FileState::Free);
            }
            continue;
        }
        return match inspect(&path) {
            Ok(RootKind::Absent) => Ok(FileState::Free),
            Ok(RootKind::File) => Ok(FileState::Taken),
            Ok(RootKind::Linked { .. }) => Err(Refusal::InnerLink { rel }),
            Ok(RootKind::Dir | RootKind::Other) => Err(Refusal::PrefixClash { rel }),
            Err(e) => Err(Refusal::Unreadable {
                rel,
                reason: e.to_string(),
            }),
        };
    }
    Ok(FileState::Free)
}

/// Both instances reaching one folder: the target root (or its nearest
/// existing ancestor, extended by the missing segments) and the source root
/// canonically equal, or one inside the other. A path that cannot be resolved
/// is "could not tell", refused as unreadable — never let through as no
/// overlap, which would let a copy write into its own source.
fn overlap(source_path: &Path, target_mc: &Path, rel: &str) -> Result<(), Refusal> {
    let unreadable = |path: &Path, why: String| Refusal::Unreadable {
        rel: rel.into(),
        reason: format!("{}: {why}", path.display()),
    };
    let source =
        dunce::canonicalize(source_path).map_err(|e| unreadable(source_path, e.to_string()))?;
    let mut existing = join_rel(target_mc, rel);
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    loop {
        match std::fs::symlink_metadata(&existing) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(unreadable(&existing, e.to_string())),
        }
        let (Some(name), Some(parent)) = (existing.file_name(), existing.parent()) else {
            return Err(unreadable(&existing, "no folder above it exists".into()));
        };
        missing.push(name.to_os_string());
        existing = parent.to_path_buf();
    }
    let mut target =
        dunce::canonicalize(&existing).map_err(|e| unreadable(&existing, e.to_string()))?;
    for seg in missing.iter().rev() {
        target.push(seg);
    }
    if target.starts_with(&source) || source.starts_with(&target) {
        return Err(Refusal::Overlap { rel: rel.into() });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::content_root::test_links::link_dir;
    use crate::instances::transfer::walk::join_rel;

    struct Pair {
        _td: tempfile::TempDir,
        src: PathBuf,
        dst: PathBuf,
    }

    fn pair() -> Pair {
        let td = tempfile::tempdir().unwrap();
        let src = td.path().join("src/.minecraft");
        let dst = td.path().join("dst/.minecraft");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        Pair { _td: td, src, dst }
    }

    fn write(base: &Path, rel: &str, bytes: &[u8]) {
        let p = join_rel(base, rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn item(p: &TransferPreview, id: TransferItemId) -> &ItemPreview {
        p.items.iter().find(|i| i.id == id).expect("item listed")
    }

    #[test]
    fn absent_items_are_omitted_and_present_ones_counted() {
        let p = pair();
        write(&p.src, "schematics/a.nbt", b"abc");
        let got = preview(&p.src, &p.dst, None).unwrap();
        assert_eq!(got.items.len(), 1);
        let s = item(&got, TransferItemId::Schematics);
        assert_eq!((s.files, s.bytes, s.conflicts), (1, 3.0, 0));
        assert!(s.refusal.is_none());
    }

    #[test]
    fn a_same_named_target_file_is_a_conflict() {
        let p = pair();
        write(&p.src, "schematics/a.nbt", b"new");
        write(&p.dst, "schematics/a.nbt", b"old");
        let got = preview(&p.src, &p.dst, None).unwrap();
        assert_eq!(item(&got, TransferItemId::Schematics).conflicts, 1);
    }

    #[test]
    fn a_target_file_where_a_folder_goes_refuses_the_item() {
        let p = pair();
        write(&p.src, "schematics/a.nbt", b"a");
        write(&p.dst, "schematics", b"I am a file");
        let got = preview(&p.src, &p.dst, None).unwrap();
        assert!(matches!(
            item(&got, TransferItemId::Schematics).refusal,
            Some(Refusal::PrefixClash { .. })
        ));
    }

    #[test]
    fn a_target_folder_where_a_file_goes_refuses_the_item() {
        let p = pair();
        write(&p.src, "schematics/a.nbt", b"a");
        std::fs::create_dir_all(p.dst.join("schematics/a.nbt")).unwrap();
        let got = preview(&p.src, &p.dst, None).unwrap();
        assert!(matches!(
            item(&got, TransferItemId::Schematics).refusal,
            Some(Refusal::PrefixClash { .. })
        ));
    }

    #[test]
    fn a_broken_source_root_is_listed_and_refused() {
        let p = pair();
        let gone = p.src.join("pool");
        std::fs::create_dir_all(&gone).unwrap();
        link_dir(&gone, &p.src.join("schematics"));
        std::fs::remove_dir(&gone).unwrap();
        let got = preview(&p.src, &p.dst, None).unwrap();
        let s = item(&got, TransferItemId::Schematics);
        assert!(matches!(s.refusal, Some(Refusal::BrokenLink { .. })));
        assert!(matches!(
            s.roots[0].source,
            ContentLocation::BrokenLink { .. }
        ));
    }

    #[test]
    fn a_linked_target_root_is_allowed_and_shown() {
        let p = pair();
        write(&p.src, "schematics/a.nbt", b"a");
        let shared = p.dst.parent().unwrap().join("shared-schematics");
        std::fs::create_dir_all(&shared).unwrap();
        link_dir(&shared, &p.dst.join("schematics"));
        let got = preview(&p.src, &p.dst, None).unwrap();
        let s = item(&got, TransferItemId::Schematics);
        assert!(s.refusal.is_none(), "{:?}", s.refusal);
        assert!(matches!(
            s.roots[0].target,
            Some(ContentLocation::Linked { .. })
        ));
    }

    #[test]
    fn a_link_inside_the_target_where_the_copy_writes_refuses_the_item() {
        let p = pair();
        write(&p.src, "schematics/sub/a.nbt", b"a");
        let elsewhere = p.dst.parent().unwrap().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::create_dir_all(p.dst.join("schematics")).unwrap();
        link_dir(&elsewhere, &p.dst.join("schematics/sub"));
        let got = preview(&p.src, &p.dst, None).unwrap();
        assert!(matches!(
            item(&got, TransferItemId::Schematics).refusal,
            Some(Refusal::InnerLink { .. })
        ));
    }

    #[test]
    fn both_instances_using_one_folder_refuses_the_item() {
        let p = pair();
        write(&p.src, "schematics/a.nbt", b"a");
        link_dir(&p.src.join("schematics"), &p.dst.join("schematics"));
        let got = preview(&p.src, &p.dst, None).unwrap();
        assert!(matches!(
            item(&got, TransferItemId::Schematics).refusal,
            Some(Refusal::Overlap { .. })
        ));
    }

    #[test]
    fn xaero_counts_multiplayer_folders_only() {
        let p = pair();
        write(&p.src, "xaero/minimap/Multiplayer_a/w.txt", b"w");
        write(&p.src, "xaero/minimap/New World/s.txt", b"s");
        let got = preview(&p.src, &p.dst, None).unwrap();
        assert_eq!(item(&got, TransferItemId::Xaero).files, 1);
    }

    #[test]
    fn other_folders_offer_unmanaged_top_level_folders() {
        let p = pair();
        write(&p.src, "kubejs/x.js", b"x");
        write(&p.src, ".replay_cache/c", b"c");
        write(&p.src, "mods/m.jar", b"m");
        write(&p.src, "Config/c.toml", b"c");
        write(&p.src, "usercache.json", b"{}");
        let got = preview(&p.src, &p.dst, None).unwrap();
        let mut names: Vec<&str> = got.other.iter().map(|o| o.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, [".replay_cache", "kubejs"]);
    }

    /// Could not tell whether both instances reach one folder: refused as
    /// unreadable, never let through as no overlap.
    #[test]
    fn an_overlap_that_cannot_be_checked_is_refused() {
        let p = pair();
        let gone = p.src.join("schematics");
        let got = overlap(&gone, &p.dst, "schematics");
        assert!(
            matches!(got, Err(Refusal::Unreadable { ref rel, .. }) if rel == "schematics"),
            "{got:?}"
        );
    }
}

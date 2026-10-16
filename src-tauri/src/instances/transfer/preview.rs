//! What the transfer dialog shows (spec §A3): for each catalog item and each
//! "Other folder" the source has, where its roots are on both sides, what would
//! be copied, how many target files share a name with it, and whether anything
//! makes the copy unsafe (§A5). Read-only on both sides.

use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::catalog::{Scope, TransferItemId};
use super::walk::Walked;
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
    let _ = (source_mc, target_mc, target_free_mb);
    todo!("red-first stub: the preview lands in the next commit")
}

/// The source game dir's top-level folders "Other folders" offers.
pub(crate) fn other_folder_names(source_mc: &Path) -> std::io::Result<Vec<String>> {
    let _ = source_mc;
    todo!("red-first stub")
}

/// One root, both sides. `None` when the source has nothing at `rel`.
pub(crate) fn scan_root(
    source_mc: &Path,
    target_mc: &Path,
    rel: &str,
    scope: Scope,
) -> Option<RootScan> {
    let _ = (source_mc, target_mc, rel, scope);
    todo!("red-first stub")
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
}

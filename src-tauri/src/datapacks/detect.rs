//! The one place that encodes Minecraft's `PackDetector` (spec §2 N.1): which
//! entries of a world's `datapacks/` folder the game loads as packs, and how a
//! name Lucerna holds maps onto an on-disk entry (N.3 R2).
//!
//! Engine facts (`javap -c` on 26.2, spec §2 N.0):
//! * a directory is a pack only if `dir/pack.mcmeta` is a regular file
//!   (`Files.isRegularFile`, which follows links);
//! * a file is a pack only if its name `endsWith(".zip")` — case-sensitive —
//!   and the zip has `pack.mcmeta` at its root (`ZipFile.getEntry`, exact);
//! * the pack id is `"file/" + getFileName()`: the on-disk spelling, exactly.
//!
//! Symlinked entries keep their pre-batch handling (§0.5 A25): classified by
//! `file_type()`, which does not follow them, so a link is never a folder
//! pack; a link named `*.zip` is root-checked through the link.
//!
//! Content verdicts are not made here (§0.5 A9): a `Pack` may still be
//! `WontLoad`/`Broken` for `compat`; A1 turns `WontLoad` into Ignored for
//! library-vouched entries, in `state::derive`.
//!
//! Synchronous (std + `zip`). Async callers run it in `spawn_blocking`.

use std::fs;
use std::io::{self, BufReader};
use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::DatapackRejection;

const PACK_MCMETA: &str = "pack.mcmeta";
const ZIP_SUFFIX: &str = ".zip";
/// The nested-folder hint looks at no more children than this. Past it the
/// hint is less specific; the verdict is not (N.10).
const NESTED_PROBE_LIMIT: usize = 16;

/// Why the game does not load an entry of a world's `datapacks/` folder.
#[derive(Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum IgnoredReason {
    /// A folder with no `pack.mcmeta` directly inside it.
    FolderWithoutPackMcmeta,
    /// The folder's `pack.mcmeta` is one level deeper.
    FolderPackNestedInside,
    /// The file name ends in `.ZIP` (or another capitalisation).
    ZipExtensionNotLowercase,
    /// The zip has no `pack.mcmeta` at its root.
    ZipWithoutPackMcmeta,
    /// Lucerna could not read the entry to check. Not a claim about the
    /// game: the UI says "Couldn't check".
    Unreadable,
    /// A well-formed pack whose `pack.mcmeta` this Minecraft cannot load
    /// (§0.5 A1). Never produced here: `state::derive` sets it from `loadable`.
    NotLoadable,
}

/// What one `datapacks/` entry is to the game. Never serialized: rows carry
/// `WorldPackState` + `IgnoredReason`, which only `state::derive` produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Pack { is_dir: bool },
    Unusable { is_dir: bool, reason: IgnoredReason },
}

impl Presence {
    #[must_use]
    pub fn is_dir(&self) -> bool {
        match *self {
            Presence::Pack { is_dir } | Presence::Unusable { is_dir, .. } => is_dir,
        }
    }
}

/// One entry of a world's `datapacks/`, named exactly as on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnDiskEntry {
    pub name: String,
    pub presence: Presence,
    /// The zip was vouched for (N.1) and never opened. §0.5 A1: `loadable`
    /// comes from the registry row only for such entries (see `state::loadable_of`).
    pub vouched: bool,
}

/// Could not tell. The cause is kept so a writer can report it (N.4).
struct Unreadable {
    is_dir: bool,
    cause: io::Error,
}

/// A24: the one root rule. Install (`pack_meta::classify`), add and listing share it.
#[must_use]
pub fn is_root_pack_mcmeta(zip_entry_name: &str) -> bool {
    zip_entry_name == PACK_MCMETA
}

/// The game's own test: `String.endsWith(".zip")`, case-sensitive.
#[must_use]
pub fn has_zip_suffix(name: &str) -> bool {
    name.ends_with(ZIP_SUFFIX)
}

/// `.zip` in any capitalisation. ASCII folding is exact: the suffix is ASCII.
fn has_zip_suffix_any_case(name: &str) -> bool {
    let n = name.len();
    let k = ZIP_SUFFIX.len();
    n >= k && name.is_char_boundary(n - k) && name[n - k..].eq_ignore_ascii_case(ZIP_SUFFIX)
}

/// N.5: `X.ZIP` (any capitalisation) becomes `X.zip`; every other name is unchanged.
#[must_use]
pub fn normalise_zip_extension(name: &str) -> String {
    if has_zip_suffix_any_case(name) && !has_zip_suffix(name) {
        format!("{}{ZIP_SUFFIX}", &name[..name.len() - ZIP_SUFFIX.len()])
    } else {
        name.to_string()
    }
}

/// Whether the zip at `path` has `pack.mcmeta` at its root. Reads the central
/// directory only (entry names, no decompression). `Err` = could not tell.
pub fn zip_has_root_pack_mcmeta(path: &Path) -> io::Result<bool> {
    let file = fs::File::open(path)?;
    let archive = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let found = archive.file_names().any(is_root_pack_mcmeta);
    Ok(found)
}

/// `Ok(None)` = not an entry the game considers (a plain non-zip file; the
/// game logs `Found non-pack entry` and we do not list it).
fn classify_at(
    path: &Path,
    name: &str,
    file_type: io::Result<fs::FileType>,
    vouched: bool,
) -> Result<Option<Presence>, Unreadable> {
    let ft = file_type.map_err(|cause| Unreadable {
        is_dir: false,
        cause,
    })?;
    if ft.is_dir() {
        return folder_presence(path).map(Some);
    }
    if has_zip_suffix(name) {
        if vouched {
            return Ok(Some(Presence::Pack { is_dir: false }));
        }
        return match zip_has_root_pack_mcmeta(path) {
            Ok(true) => Ok(Some(Presence::Pack { is_dir: false })),
            Ok(false) => Ok(Some(Presence::Unusable {
                is_dir: false,
                reason: IgnoredReason::ZipWithoutPackMcmeta,
            })),
            Err(cause) => Err(Unreadable {
                is_dir: false,
                cause,
            }),
        };
    }
    if has_zip_suffix_any_case(name) {
        return Ok(Some(Presence::Unusable {
            is_dir: false,
            reason: IgnoredReason::ZipExtensionNotLowercase,
        }));
    }
    Ok(None)
}

fn folder_presence(dir: &Path) -> Result<Presence, Unreadable> {
    match fs::metadata(dir.join(PACK_MCMETA)) {
        Ok(meta) if meta.is_file() => return Ok(Presence::Pack { is_dir: true }),
        // Present but not a regular file (a directory named pack.mcmeta): the
        // game's `isRegularFile` is false, so this is not a pack either.
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(cause) => {
            return Err(Unreadable {
                is_dir: true,
                cause,
            })
        }
    }
    let reason = if nested_pack_inside(dir) {
        IgnoredReason::FolderPackNestedInside
    } else {
        IgnoredReason::FolderWithoutPackMcmeta
    };
    Ok(Presence::Unusable {
        is_dir: true,
        reason,
    })
}

/// Whether one of the folder's first `NESTED_PROBE_LIMIT` children holds the
/// pack. Only the hint depends on this. The verdict (no `pack.mcmeta` here)
/// is already proven, so a failed probe read falls back to the less specific
/// `FolderWithoutPackMcmeta` (N.1).
fn nested_pack_inside(dir: &Path) -> bool {
    let Ok(children) = fs::read_dir(dir) else {
        return false; // the verdict stands; only the hint gets less specific
    };
    children.take(NESTED_PROBE_LIMIT).flatten().any(|child| {
        child.file_type().is_ok_and(|ft| ft.is_dir())
            && fs::metadata(child.path().join(PACK_MCMETA)).is_ok_and(|m| m.is_file())
    })
}

fn unreadable(path: &Path, u: Unreadable) -> Presence {
    crate::diag!(
        "datapacks: could not read {} to check whether Minecraft loads it: {}",
        path.display(),
        u.cause
    );
    Presence::Unusable {
        is_dir: u.is_dir,
        reason: IgnoredReason::Unreadable,
    }
}

/// Every entry of `dp_dir` the game considers, classified as `PackDetector`
/// does. `vouch(name, world_path) == true` lets a `.zip` skip its root check
/// (N.1 vouch rule; the server always passes `|_, _| false`). A missing
/// folder is no entries. Any other read failure is `Err` (could not tell).
pub fn scan(dp_dir: &Path, vouch: &dyn Fn(&str, &Path) -> bool) -> io::Result<Vec<OnDiskEntry>> {
    let entries = match fs::read_dir(dp_dir) {
        Ok(rd) => rd,
        // A world that never had a pack: a fact, not ignorance.
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let file_type = entry.file_type();
        let vouched = has_zip_suffix(&name)
            && file_type.as_ref().is_ok_and(|ft| !ft.is_dir())
            && vouch(&name, &path);
        let presence = match classify_at(&path, &name, file_type, vouched) {
            Ok(Some(p)) => p,
            Ok(None) => continue,
            Err(u) => unreadable(&path, u),
        };
        out.push(OnDiskEntry {
            name,
            presence,
            vouched,
        });
    }
    Ok(out)
}

/// The names in `dp_dir` from one `read_dir`: A19's writer input for `resolve`.
/// A missing folder is no names.
pub fn entry_names(dp_dir: &Path) -> io::Result<Vec<String>> {
    match fs::read_dir(dp_dir) {
        Ok(rd) => rd
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// A19: classify only the named entry (`symlink_metadata`, not followed, like
/// `DirEntry::file_type`). `None` = a plain non-zip file. An unreadable entry
/// reads as `Unusable(Unreadable)` and is logged.
#[must_use]
pub fn classify_one(dp_dir: &Path, name: &str, vouched: bool) -> Option<Presence> {
    let path = dp_dir.join(name);
    let file_type = fs::symlink_metadata(&path).map(|m| m.file_type());
    match classify_at(&path, name, file_type, vouched) {
        Ok(p) => p,
        Err(u) => Some(unreadable(&path, u)),
    }
}

/// N.3 R2: does `name` denote an on-disk entry?
#[derive(Debug)]
pub enum Resolved {
    Exact(String),
    /// Exactly one entry differs from `name` only in case, and the file
    /// system resolves `name` to it (NTFS, default APFS).
    Folded(String),
    Absent,
    /// A stat error other than NotFound: could not tell.
    Unknown(io::Error),
}

/// R2. Costs a stat only for a name that drifts. `names` come from one `read_dir`.
pub fn resolve(dp_dir: &Path, name: &str, names: &[String]) -> Resolved {
    if names.iter().any(|n| n == name) {
        return Resolved::Exact(name.to_string());
    }
    let folded = name.to_lowercase();
    let mut matches = names.iter().filter(|n| n.to_lowercase() == folded);
    let (Some(only), None) = (matches.next(), matches.next()) else {
        return Resolved::Absent;
    };
    match fs::symlink_metadata(dp_dir.join(name)) {
        Ok(_) => Resolved::Folded(only.clone()),
        // Case-sensitive file system: a different pack.
        Err(e) if e.kind() == io::ErrorKind::NotFound => Resolved::Absent,
        Err(e) => Resolved::Unknown(e),
    }
}

/// R2 for the display merge. `Unknown` joins nothing (the name stands as its
/// own row) and is logged.
#[must_use]
pub fn resolve_for_display(dp_dir: &Path, name: &str, names: &[String]) -> Option<String> {
    match resolve(dp_dir, name, names) {
        Resolved::Exact(n) | Resolved::Folded(n) => Some(n),
        Resolved::Absent => None,
        Resolved::Unknown(e) => {
            crate::diag!(
                "datapacks: could not tell whether {name} is in {}: {e}; listed as its own row",
                dp_dir.display()
            );
            None
        }
    }
}

/// What a toggle is about to act on (N.4, A19).
#[derive(Debug)]
pub enum WriteTarget {
    Absent,
    /// `name` is the on-disk spelling; `presence` is `None` for a plain non-zip file.
    Present {
        name: String,
        presence: Option<Presence>,
    },
}

/// R2 against one `read_dir`, then classify only the resolved entry, with no
/// vouch. `Err` = could not tell (`Unknown`, or an unreadable entry), with its cause.
pub fn target_for_write(dp_dir: &Path, name: &str) -> io::Result<WriteTarget> {
    let names = entry_names(dp_dir)?;
    let on_disk = match resolve(dp_dir, name, &names) {
        Resolved::Exact(n) | Resolved::Folded(n) => n,
        Resolved::Absent => return Ok(WriteTarget::Absent),
        Resolved::Unknown(e) => return Err(e),
    };
    let path = dp_dir.join(&on_disk);
    let file_type = fs::symlink_metadata(&path).map(|m| m.file_type());
    let presence = classify_at(&path, &on_disk, file_type, false).map_err(|u| u.cause)?;
    Ok(WriteTarget::Present {
        name: on_disk,
        presence,
    })
}

/// N.4: the typed refusal for a write aimed at an entry the game ignores.
#[must_use]
pub fn rejection_for(reason: IgnoredReason) -> DatapackRejection {
    match reason {
        IgnoredReason::ZipExtensionNotLowercase => DatapackRejection::NotAZip,
        IgnoredReason::FolderWithoutPackMcmeta
        | IgnoredReason::FolderPackNestedInside
        | IgnoredReason::ZipWithoutPackMcmeta => DatapackRejection::NotAPack,
        // `target_for_write` reports an unreadable entry as `Err`, and
        // `NotLoadable` comes only from `state::derive`. If that ever changes,
        // refusing is still the restrictive answer.
        IgnoredReason::Unreadable | IgnoredReason::NotLoadable => DatapackRejection::NotAPack,
    }
}

/// N.3 provenance lookup after a successful file-system read: the exact name
/// first, else the single case-insensitive match (none when two match).
pub fn find_by_name<'a, T>(rows: &'a [T], name: &str, key: impl Fn(&T) -> &str) -> Option<&'a T> {
    if let Some(r) = rows.iter().find(|r| key(r) == name) {
        return Some(r);
    }
    let folded = name.to_lowercase();
    let mut it = rows.iter().filter(|r| key(r).to_lowercase() == folded);
    match (it.next(), it.next()) {
        (Some(r), None) => Some(r),
        _ => None,
    }
}

/// One row of the display merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedRow {
    /// The row's identity: the on-disk spelling, or the source's own spelling
    /// when nothing is on disk. Its exact `file/` id is the engine's id (R1).
    pub filename: String,
    /// Index into the `on_disk` slice.
    pub on_disk: Option<usize>,
    /// The registry/sidecar name that joined this row.
    pub joined: Option<String>,
}

/// N.3 display merge. On-disk entries never merge with each other. `joined`
/// names (registry or sidecar) join an on-disk row through `resolves_to` (R2).
/// A level.dat name joins an on-disk row when it is exact, or when exactly one
/// on-disk entry matches it case-insensitively (a stale spelling the engine
/// drops; hiding it states nothing false, because the row's state uses the exact
/// id). Everything else is exact. Sorted case-insensitively for display.
pub fn display_merge(
    on_disk: &[OnDiskEntry],
    joined: &[String],
    resolves_to: &dyn Fn(&str) -> Option<String>,
    level_dat_names: &[String],
) -> Vec<MergedRow> {
    let mut rows: Vec<MergedRow> = on_disk
        .iter()
        .enumerate()
        .map(|(i, e)| MergedRow {
            filename: e.name.clone(),
            on_disk: Some(i),
            joined: None,
        })
        .collect();
    for n in joined {
        let target = resolves_to(n).unwrap_or_else(|| n.clone());
        match rows.iter_mut().find(|r| r.filename == target) {
            Some(r) => r.joined = Some(n.clone()),
            None => rows.push(MergedRow {
                filename: n.clone(),
                on_disk: None,
                joined: Some(n.clone()),
            }),
        }
    }
    for n in level_dat_names {
        if rows.iter().any(|r| &r.filename == n) {
            continue;
        }
        let folded = n.to_lowercase();
        let mut ci = on_disk.iter().filter(|e| e.name.to_lowercase() == folded);
        if let (Some(_), None) = (ci.next(), ci.next()) {
            continue;
        }
        rows.push(MergedRow {
            filename: n.clone(),
            on_disk: None,
            joined: None,
        });
    }
    rows.sort_by(|a, b| {
        a.filename
            .to_lowercase()
            .cmp(&b.filename.to_lowercase())
            .then_with(|| a.filename.cmp(&b.filename))
    });
    rows
}

#[cfg(test)]
pub mod test_support {
    //! `pub mod`, not `pub(crate) mod` (§0.5 A17): the in-place-write guard
    //! masks only `mod`/`pub mod` test regions, and these helpers write files.
    use std::io::Write;
    use std::path::Path;

    /// A zip holding exactly these entries, stored uncompressed.
    pub fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, body) in entries {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(body).unwrap();
        }
        zw.finish().unwrap().into_inner()
    }

    /// A datapack the game loads: root `pack.mcmeta` with a description
    /// (§0.2 I9) and a `data/` tree.
    pub fn pack_zip() -> Vec<u8> {
        zip_of(&[
            (
                "pack.mcmeta",
                br#"{"pack":{"pack_format":48,"description":"Test pack"}}"#,
            ),
            ("data/t/function/a.mcfunction", b"say hi"),
        ])
    }

    /// Whether `dir`'s file system resolves a name in another case to the
    /// same entry (NTFS, default APFS). Spec §2 N.8's platform probe.
    pub fn fs_folds_case(dir: &Path) -> bool {
        let probe = dir.join("lucerna-case-probe");
        std::fs::write(&probe, b"").expect("write the case probe");
        let folds = dir.join("LUCERNA-CASE-PROBE").exists();
        std::fs::remove_file(&probe).expect("remove the case probe");
        folds
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    fn never(_: &str, _: &Path) -> bool {
        false
    }
    fn scan_one(dir: &Path) -> OnDiskEntry {
        let mut v = scan(dir, &never).unwrap();
        assert_eq!(v.len(), 1, "{v:?}");
        v.remove(0)
    }
    fn entry(name: &str) -> OnDiskEntry {
        OnDiskEntry {
            name: name.into(),
            presence: Presence::Pack { is_dir: false },
            vouched: false,
        }
    }

    #[test]
    fn a_folder_with_pack_mcmeta_is_a_pack() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(td.path().join("Real/data")).unwrap();
        std::fs::write(td.path().join("Real/pack.mcmeta"), b"{}").unwrap();
        assert_eq!(
            scan_one(td.path()).presence,
            Presence::Pack { is_dir: true }
        );
    }

    #[test]
    fn folder_without_pack_mcmeta_is_ignored_with_that_reason() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(td.path().join("Loose/data")).unwrap();
        assert_eq!(
            scan_one(td.path()).presence,
            Presence::Unusable {
                is_dir: true,
                reason: IgnoredReason::FolderWithoutPackMcmeta
            }
        );
    }

    #[test]
    fn nested_folder_pack_is_ignored_as_nested() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(td.path().join("Outer/Inner/data")).unwrap();
        std::fs::write(td.path().join("Outer/Inner/pack.mcmeta"), b"{}").unwrap();
        assert_eq!(
            scan_one(td.path()).presence,
            Presence::Unusable {
                is_dir: true,
                reason: IgnoredReason::FolderPackNestedInside
            }
        );
    }

    #[test]
    fn a_pack_mcmeta_that_is_a_directory_does_not_make_a_pack() {
        // `Files.isRegularFile` — a directory named pack.mcmeta is not one.
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(td.path().join("Odd/pack.mcmeta")).unwrap();
        assert!(matches!(
            scan_one(td.path()).presence,
            Presence::Unusable { is_dir: true, .. }
        ));
    }

    #[test]
    fn upper_case_zip_is_ignored_by_its_extension() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("Pack.ZIP"), pack_zip()).unwrap();
        assert_eq!(
            scan_one(td.path()).presence,
            Presence::Unusable {
                is_dir: false,
                reason: IgnoredReason::ZipExtensionNotLowercase
            }
        );
    }

    #[test]
    fn zip_without_root_pack_mcmeta_is_ignored() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(
            td.path().join("rootless.zip"),
            zip_of(&[("Inner/pack.mcmeta", b"{}"), ("Inner/data/x", b"")]),
        )
        .unwrap();
        assert_eq!(
            scan_one(td.path()).presence,
            Presence::Unusable {
                is_dir: false,
                reason: IgnoredReason::ZipWithoutPackMcmeta
            }
        );
    }

    #[test]
    fn a_dot_slash_pack_mcmeta_is_not_at_the_root() {
        // `ZipFile.getEntry("pack.mcmeta")` is exact (§0.5 A24).
        let td = tempfile::tempdir().unwrap();
        std::fs::write(
            td.path().join("dot.zip"),
            zip_of(&[("./pack.mcmeta", b"{}"), ("data/x", b"")]),
        )
        .unwrap();
        assert_eq!(
            scan_one(td.path()).presence,
            Presence::Unusable {
                is_dir: false,
                reason: IgnoredReason::ZipWithoutPackMcmeta
            }
        );
    }

    #[test]
    fn non_zip_named_zip_is_unreadable() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("broken.zip"), b"not a zip").unwrap();
        assert_eq!(
            scan_one(td.path()).presence,
            Presence::Unusable {
                is_dir: false,
                reason: IgnoredReason::Unreadable
            }
        );
    }

    #[test]
    fn a_vouched_zip_is_never_opened() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("lib.zip"), b"garbage the zip crate rejects").unwrap();
        let v = scan(td.path(), &|name: &str, _: &Path| name == "lib.zip").unwrap();
        assert_eq!(
            v,
            vec![OnDiskEntry {
                name: "lib.zip".into(),
                presence: Presence::Pack { is_dir: false },
                vouched: true
            }]
        );
    }

    #[test]
    fn other_files_are_not_entries() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("readme.txt"), b"x").unwrap();
        std::fs::write(td.path().join("pack.zip.disabled"), b"x").unwrap();
        assert!(scan(td.path(), &never).unwrap().is_empty());
    }

    #[test]
    fn a_missing_datapacks_folder_has_no_entries() {
        let td = tempfile::tempdir().unwrap();
        assert!(scan(&td.path().join("datapacks"), &never)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_datapacks_path_that_is_a_file_is_an_error() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("datapacks"), b"not a dir").unwrap();
        assert!(scan(&td.path().join("datapacks"), &never).is_err());
    }

    #[test]
    fn normalise_zip_extension_fixes_only_the_extension() {
        assert_eq!(normalise_zip_extension("Pack.ZIP"), "Pack.zip");
        assert_eq!(normalise_zip_extension("Пак.Zip"), "Пак.zip");
        assert_eq!(normalise_zip_extension("ok.zip"), "ok.zip");
        assert_eq!(normalise_zip_extension("ZIP"), "ZIP");
        assert_eq!(normalise_zip_extension("pack.jar"), "pack.jar");
    }

    #[test]
    fn resolve_prefers_the_exact_name() {
        let td = tempfile::tempdir().unwrap();
        let names = vec!["vm.zip".to_string()];
        assert!(
            matches!(resolve(td.path(), "vm.zip", &names), Resolved::Exact(n) if n == "vm.zip")
        );
        assert!(matches!(
            resolve(td.path(), "other.zip", &names),
            Resolved::Absent
        ));
    }

    #[test]
    fn resolve_folds_only_where_the_file_system_does() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("veinminer.zip"), b"x").unwrap();
        let names = vec!["veinminer.zip".to_string()];
        let got = resolve(td.path(), "VeinMiner.zip", &names);
        if fs_folds_case(td.path()) {
            assert!(
                matches!(got, Resolved::Folded(ref n) if n == "veinminer.zip"),
                "{got:?}"
            );
        } else {
            assert!(matches!(got, Resolved::Absent), "{got:?}");
        }
    }

    #[test]
    fn resolve_never_picks_between_two_case_variants() {
        let td = tempfile::tempdir().unwrap();
        let names = vec!["Foo.zip".to_string(), "foo.zip".to_string()];
        assert!(matches!(
            resolve(td.path(), "FOO.zip", &names),
            Resolved::Absent
        ));
    }

    #[test]
    fn two_entries_differing_only_in_case_are_two_rows() {
        let rows = display_merge(
            &[entry("Foo.zip"), entry("foo.zip")],
            &[],
            &|_: &str| None,
            &[],
        );
        let names: Vec<&str> = rows.iter().map(|r| r.filename.as_str()).collect();
        assert_eq!(names, vec!["Foo.zip", "foo.zip"]);
    }

    #[test]
    fn a_level_dat_spelling_joins_its_single_on_disk_match() {
        // A stale spelling the engine drops is hidden; the row's state uses the exact id.
        let rows = display_merge(
            &[entry("veinminer.zip")],
            &[],
            &|_: &str| None,
            &["VeinMiner.zip".into(), "ghost.zip".into()],
        );
        let names: Vec<&str> = rows.iter().map(|r| r.filename.as_str()).collect();
        assert_eq!(names, vec!["ghost.zip", "veinminer.zip"]);
        // Two case variants on disk: the level.dat name joins neither.
        let rows = display_merge(
            &[entry("Foo.zip"), entry("foo.zip")],
            &[],
            &|_: &str| None,
            &["FOO.zip".into()],
        );
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn a_registry_name_joins_only_through_resolution() {
        let joined = vec!["VeinMiner.zip".to_string()];
        let folds = |n: &str| (n == "VeinMiner.zip").then(|| "veinminer.zip".to_string());
        let rows = display_merge(&[entry("veinminer.zip")], &joined, &folds, &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "veinminer.zip");
        assert_eq!(rows[0].joined.as_deref(), Some("VeinMiner.zip"));
        let rows = display_merge(&[entry("veinminer.zip")], &joined, &|_: &str| None, &[]);
        assert_eq!(rows.len(), 2, "case-sensitive: two packs");
    }

    #[test]
    fn find_by_name_prefers_exact_then_a_single_folded_match() {
        let rows = vec![
            "Terra.zip".to_string(),
            "terra.zip".to_string(),
            "Solo.zip".to_string(),
        ];
        // A fn item, not a closure: a closure cannot name the higher-ranked
        // lifetime `find_by_name`'s `Fn(&T) -> &str` needs.
        let key = String::as_str;
        assert_eq!(
            find_by_name(&rows, "terra.zip", key).map(String::as_str),
            Some("terra.zip")
        );
        assert_eq!(
            find_by_name(&rows, "TERRA.zip", key),
            None,
            "two folded matches: no guess"
        );
        assert_eq!(
            find_by_name(&rows, "solo.ZIP", key).map(String::as_str),
            Some("Solo.zip")
        );
    }

    #[test]
    fn target_for_write_classifies_only_the_resolved_entry() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(td.path().join("Loose/data")).unwrap();
        std::fs::write(td.path().join("notes.txt"), b"x").unwrap();
        assert!(matches!(
            target_for_write(td.path(), "Loose").unwrap(),
            WriteTarget::Present { ref name, presence: Some(Presence::Unusable { reason: IgnoredReason::FolderWithoutPackMcmeta, .. }) } if name == "Loose"
        ));
        assert!(matches!(
            target_for_write(td.path(), "notes.txt").unwrap(),
            WriteTarget::Present { presence: None, .. }
        ));
        assert!(matches!(
            target_for_write(td.path(), "missing.zip").unwrap(),
            WriteTarget::Absent
        ));
    }
}

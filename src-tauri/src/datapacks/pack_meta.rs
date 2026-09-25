//! Classify a pack zip and read `pack.mcmeta`.
//!
//! `pack.mcmeta` alone does NOT identify a datapack — every resource pack has
//! one too. The discriminator is the top-level tree: `data/` for a datapack,
//! `assets/` for a resource pack. A combined pack shipping both is treated as a
//! datapack, because that is the tree Minecraft loads from `datapacks/`.
//! `pack.mcmeta` is mandatory either way: a `data/`-only zip with no
//! `pack.mcmeta` is `Neither`, not a datapack.
//!
//! Everything here is best-effort by design: an unreadable zip is `Neither`
//! and unreadable metadata is `PackMcmeta::Unreadable`, never an error. Same house style as
//! `mods::local::read_jar_meta`.

use std::io::{Read, Seek};
use std::path::Path;

use crate::datapacks::format::{self, PackMcmeta};

/// What a pack zip's top-level tree identifies it as. See the module doc for
/// the exact discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    /// Root `pack.mcmeta` + a top-level `data/` tree (present even if `assets/`
    /// is also present — a combined pack loads as a datapack).
    Datapack,
    /// Root `pack.mcmeta` + a top-level `assets/` tree and no `data/` tree.
    ResourcePack,
    /// Not a readable zip, missing `pack.mcmeta`, or missing both trees.
    Neither,
}

/// What Lucerna reads from a pack zip's `pack.mcmeta`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackMeta {
    /// What the pack declares — facts for `datapacks::verdict`, never a verdict.
    pub mcmeta: PackMcmeta,
    /// Plain text of `pack.description` (rich text flattened, `§` codes
    /// stripped); `None` when absent or empty. The pack's display name.
    pub description: Option<String>,
}

impl PackMeta {
    fn unreadable() -> Self {
        Self {
            mcmeta: PackMcmeta::Unreadable,
            description: None,
        }
    }
}

/// True when `name` (already stripped of a leading `./`) is a direct child of
/// the zip root, under `dir` — i.e. `dir` itself or a path beginning `dir/`.
fn under_top_level(name: &str, dir: &str) -> bool {
    name.strip_prefix(dir)
        .map(|rest| rest.starts_with('/'))
        .unwrap_or(false)
}

/// Classify a pack zip by its top-level tree. Reads only the central
/// directory (via [`zip::ZipArchive::file_names`]) rather than opening each
/// entry: opening would re-parse each local header and set up a decompressing
/// reader we don't need just to look at a name, and would error out (silently
/// dropping the entry from classification) on an encrypted entry or a
/// compression method this build lacks. Discarding the result is a bug —
/// `Neither` on an unreadable zip is a real, meaningful answer.
#[must_use]
pub fn classify(bytes: &[u8]) -> PackKind {
    let Ok(zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return PackKind::Neither;
    };
    let mut has_meta = false;
    let mut has_data = false;
    let mut has_assets = false;
    for name in zip.file_names() {
        // §0.5 A24: the root rule is `detect`'s, exact, with no `./`
        // trimming — the game's `ZipFile.getEntry("pack.mcmeta")` is exact.
        if crate::datapacks::detect::is_root_pack_mcmeta(name) {
            has_meta = true;
            continue;
        }
        let name = name.trim_start_matches("./");
        if under_top_level(name, "data") {
            has_data = true;
        } else if under_top_level(name, "assets") {
            has_assets = true;
        }
        // Datapack is the highest-priority outcome and already proven; a
        // resource pack still needs the full scan to rule out a `data/` tree
        // appearing later in the central directory.
        if has_meta && has_data {
            break;
        }
    }
    match (has_meta, has_data, has_assets) {
        (true, true, _) => PackKind::Datapack,
        (true, false, true) => PackKind::ResourcePack,
        _ => PackKind::Neither,
    }
}

/// Read `pack.mcmeta` out of an in-memory pack zip. Never an error.
#[must_use]
pub fn read_meta(bytes: &[u8]) -> PackMeta {
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return PackMeta::unreadable();
    };
    // In memory there is no I/O that can fail: an "I/O" error out of a cursor
    // is a corrupt archive, a stable fact about the bytes — Unreadable too.
    meta_from_archive(&mut zip).unwrap_or_else(|_| PackMeta::unreadable())
}

/// Read `pack.mcmeta` out of the pack zip at `path`, file-backed: only the
/// central directory and that one entry are read. Tells the two failure
/// kinds apart (Fallback discipline, question 2): `Ok` carries a real
/// answer about the bytes (`Missing`, `Unreadable`, a declaration); `Err` is
/// "could not tell" — the file could not be opened or read, or is not a
/// regular file — and callers must not record it as an answer.
pub fn read_meta_file(path: &Path) -> std::io::Result<PackMeta> {
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(zip) => zip,
        Err(zip::result::ZipError::Io(e)) => return Err(e),
        // Not a zip (or one this crate cannot open): a fact about the bytes.
        Err(_) => return Ok(PackMeta::unreadable()),
    };
    meta_from_archive(&mut zip)
}

fn meta_from_archive<R: Read + Seek>(zip: &mut zip::ZipArchive<R>) -> std::io::Result<PackMeta> {
    let mut entry = match zip.by_name("pack.mcmeta") {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => {
            return Ok(PackMeta {
                mcmeta: PackMcmeta::Missing,
                description: None,
            })
        }
        Err(zip::result::ZipError::Io(e)) => return Err(e),
        Err(_) => return Ok(PackMeta::unreadable()),
    };
    let mut body = Vec::new();
    match entry.read_to_end(&mut body) {
        Ok(_) => {}
        // A corrupt entry (bad deflate stream) is a fact about the bytes.
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => return Ok(PackMeta::unreadable()),
        Err(e) => return Err(e),
    }
    let read = format::read_mcmeta_bytes(&body);
    Ok(PackMeta {
        mcmeta: read.mcmeta,
        description: read.name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datapacks::format::{Fact, PackMcmeta};
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in entries {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(bytes).unwrap();
        }
        zw.finish().unwrap().into_inner()
    }

    const MCMETA: &[u8] = br#"{"pack":{"pack_format":48,"description":"Vein Miner"}}"#;

    #[test]
    fn a_datapack_has_pack_mcmeta_and_a_data_dir() {
        let z = zip_with(&[
            ("pack.mcmeta", MCMETA),
            ("data/vm/function/tick.mcfunction", b"say hi"),
        ]);
        assert_eq!(classify(&z), PackKind::Datapack);
    }

    #[test]
    fn a_resource_pack_is_not_a_datapack() {
        let z = zip_with(&[
            ("pack.mcmeta", MCMETA),
            ("assets/minecraft/textures/x.png", b"\x89PNG"),
        ]);
        assert_eq!(classify(&z), PackKind::ResourcePack);
    }

    #[test]
    fn a_pack_with_both_trees_counts_as_a_datapack() {
        let z = zip_with(&[
            ("pack.mcmeta", MCMETA),
            ("data/x/function/a.mcfunction", b"say"),
            ("assets/x/textures/b.png", b"\x89PNG"),
        ]);
        assert_eq!(classify(&z), PackKind::Datapack);
    }

    #[test]
    fn a_zip_without_pack_mcmeta_is_neither() {
        let z = zip_with(&[("data/x/function/a.mcfunction", b"say")]);
        assert_eq!(classify(&z), PackKind::Neither);
    }

    #[test]
    fn unreadable_bytes_are_neither() {
        assert_eq!(classify(b"not a zip at all"), PackKind::Neither);
    }

    #[test]
    fn a_bare_data_directory_entry_still_counts_as_a_data_tree() {
        // The discriminator is tree SHAPE, not whether the tree has files in it.
        let z = zip_with(&[("pack.mcmeta", MCMETA), ("data/", b"")]);
        assert_eq!(classify(&z), PackKind::Datapack);
    }

    #[test]
    fn reads_the_declaration_and_the_plain_text_name() {
        let z = zip_with(&[("pack.mcmeta", MCMETA), ("data/x/f.mcfunction", b"")]);
        let m = read_meta(&z);
        assert!(
            matches!(&m.mcmeta, PackMcmeta::Read(d) if d.pack_format == Fact::Present(48)),
            "{:?}",
            m.mcmeta
        );
        assert_eq!(m.description.as_deref(), Some("Vein Miner"));
    }

    #[test]
    fn a_rich_text_description_is_named_by_its_plain_text() {
        // Was `a_non_string_description_is_dropped_not_stringified`: the game
        // renders rich text, so the name is its plain text (§1 C2).
        let z = zip_with(&[
            (
                "pack.mcmeta",
                br#"{"pack":{"pack_format":48,"description":[{"text":"a"}]}}"#,
            ),
            ("data/x/f.mcfunction", b""),
        ]);
        assert_eq!(read_meta(&z).description.as_deref(), Some("a"));
    }

    #[test]
    fn garbage_bytes_are_unreadable_never_an_error() {
        let m = read_meta(b"garbage");
        assert_eq!(m.mcmeta, PackMcmeta::Unreadable);
        assert_eq!(m.description, None);
    }

    #[test]
    fn a_zip_without_pack_mcmeta_is_missing() {
        let z = zip_with(&[("data/x/f.mcfunction", b"")]);
        assert_eq!(read_meta(&z).mcmeta, PackMcmeta::Missing);
    }

    #[test]
    fn a_file_backed_read_matches_the_in_memory_read() {
        let td = tempfile::tempdir().unwrap();
        let z = zip_with(&[("pack.mcmeta", MCMETA), ("data/x/f.mcfunction", b"")]);
        std::fs::write(td.path().join("p.zip"), &z).unwrap();
        assert_eq!(
            read_meta_file(&td.path().join("p.zip")).unwrap(),
            read_meta(&z)
        );
    }

    #[test]
    fn a_non_zip_file_is_unreadable_not_an_io_failure() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("p.zip"), b"PACK").unwrap();
        assert_eq!(
            read_meta_file(&td.path().join("p.zip")).unwrap().mcmeta,
            PackMcmeta::Unreadable
        );
    }

    #[test]
    fn a_directory_or_a_missing_file_is_an_io_failure_not_an_answer() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir(td.path().join("d.zip")).unwrap();
        assert!(read_meta_file(&td.path().join("d.zip")).is_err());
        assert!(read_meta_file(&td.path().join("gone.zip")).is_err());
    }
}

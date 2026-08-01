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
//! and unreadable metadata is all-`None`, never an error. Same house style as
//! `mods::local::read_jar_meta`.

use std::io::Read;

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

/// Fields read from a pack's `pack.mcmeta`. Every field is `None` when the
/// zip is unreadable or the field is absent/malformed — never an error.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackMeta {
    /// `pack.pack_format`.
    pub pack_format: Option<u32>,
    /// `pack.description`, only when it is a plain JSON string (a raw text
    /// component is dropped rather than half-rendered — see `read_meta`).
    pub description: Option<String>,
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
        let name = name.trim_start_matches("./");
        if name == "pack.mcmeta" {
            has_meta = true;
        } else if under_top_level(name, "data") {
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

/// Read `pack.mcmeta` out of a pack zip. Best-effort at two different levels:
/// a document-level failure (unreadable zip, missing entry, invalid UTF-8,
/// invalid JSON) returns [`PackMeta::default`] immediately; a per-field shape
/// mismatch (e.g. a non-string `description`) instead degrades only that
/// field to `None` through the `.and_then` chain below, leaving any other
/// field that parsed fine intact. Neither case is ever an error.
#[must_use]
pub fn read_meta(bytes: &[u8]) -> PackMeta {
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return PackMeta::default();
    };
    let Ok(mut entry) = zip.by_name("pack.mcmeta") else {
        return PackMeta::default();
    };
    let mut text = String::new();
    if entry.read_to_string(&mut text).is_err() {
        return PackMeta::default();
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return PackMeta::default();
    };
    let pack = v.get("pack");
    PackMeta {
        pack_format: pack
            .and_then(|p| p.get("pack_format"))
            .and_then(|f| f.as_u64())
            .and_then(|f| u32::try_from(f).ok()),
        // A description can also be a raw JSON text component; we only surface
        // the plain-string form rather than half-rendering rich text.
        description: pack
            .and_then(|p| p.get("description"))
            .and_then(|d| d.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn reads_pack_format_and_description() {
        let z = zip_with(&[("pack.mcmeta", MCMETA), ("data/x/f.mcfunction", b"")]);
        let m = read_meta(&z);
        assert_eq!(m.pack_format, Some(48));
        assert_eq!(m.description.as_deref(), Some("Vein Miner"));
    }

    #[test]
    fn a_non_string_description_is_dropped_not_stringified() {
        let z = zip_with(&[
            (
                "pack.mcmeta",
                br#"{"pack":{"pack_format":48,"description":[{"text":"a"}]}}"#,
            ),
            ("data/x/f.mcfunction", b""),
        ]);
        assert_eq!(read_meta(&z).description, None);
        assert_eq!(read_meta(&z).pack_format, Some(48));
    }

    #[test]
    fn missing_meta_yields_all_none_never_an_error() {
        let m = read_meta(b"garbage");
        assert_eq!(m.pack_format, None);
        assert_eq!(m.description, None);
    }
}

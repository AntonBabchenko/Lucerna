//! Classify a pack zip and read `pack.mcmeta`.
//!
//! `pack.mcmeta` alone does NOT identify a datapack — every resource pack has
//! one too. The discriminator is the top-level tree: `data/` for a datapack,
//! `assets/` for a resource pack. A combined pack shipping both is treated as a
//! datapack, because that is the tree Minecraft loads from `datapacks/`.
//!
//! Everything here is best-effort by design: an unreadable zip is `Neither`
//! and unreadable metadata is all-`None`, never an error. Same house style as
//! `mods::local::read_jar_meta`.

use std::io::Read;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    Datapack,
    ResourcePack,
    Neither,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackMeta {
    pub pack_format: Option<u32>,
    pub description: Option<String>,
}

/// True when `name` is a direct child of the zip root, under `dir`.
fn under_top_level(name: &str, dir: &str) -> bool {
    let n = name.trim_start_matches("./");
    n.strip_prefix(dir)
        .map(|rest| rest.starts_with('/'))
        .unwrap_or(false)
}

pub fn classify(bytes: &[u8]) -> PackKind {
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return PackKind::Neither;
    };
    let mut has_meta = false;
    let mut has_data = false;
    let mut has_assets = false;
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index(i) else { continue };
        let name = entry.name().trim_start_matches("./").to_string();
        if name == "pack.mcmeta" {
            has_meta = true;
        } else if under_top_level(&name, "data") {
            has_data = true;
        } else if under_top_level(&name, "assets") {
            has_assets = true;
        }
    }
    match (has_meta, has_data, has_assets) {
        (true, true, _) => PackKind::Datapack,
        (true, false, true) => PackKind::ResourcePack,
        _ => PackKind::Neither,
    }
}

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

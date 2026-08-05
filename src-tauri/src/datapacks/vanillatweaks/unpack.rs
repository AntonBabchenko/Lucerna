//! Split the bundle VT builds into the individual packs it contains.
//!
//! The build endpoint always answers with one outer zip holding one inner zip
//! per selected pack, named `<pack> v<version>.zip`. That name is the pack's
//! real filename — we take it from the bundle rather than predicting it,
//! because the bundle is authoritative and a prediction is not.

use std::io::Read;

use crate::error::{Error, Result};

/// `(filename, bytes)` per inner pack, in the bundle's own order. Entries
/// that are not `.zip` files are skipped rather than fatal: one stray readme
/// must not cost the user their packs.
pub fn split_bundle(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| {
        Error::VanillaTweaksBuildFailed {
            message: format!("the downloaded bundle is not a zip: {e}"),
        }
    })?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut entry = match zip.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                crate::diag!("vanillatweaks: unreadable bundle entry {i}: {e}");
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }
        // Reduce to the bare file name: these names come off the network and
        // become file names on disk, so a traversing entry must not survive
        // the trip. `install_named_at` / `install_bytes` check the name again
        // downstream — this is the first of two, not the only one.
        let Some(name) = std::path::Path::new(entry.name())
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
        else {
            continue;
        };
        if !name.to_ascii_lowercase().ends_with(".zip") {
            continue;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        if let Err(e) = entry.read_to_end(&mut buf) {
            crate::diag!("vanillatweaks: could not read bundle entry {name}: {e}");
            continue;
        }
        out.push((name, buf));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn bundle(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zw = ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, body) in entries {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(body).unwrap();
        }
        zw.finish().unwrap().into_inner()
    }

    #[test]
    fn splits_every_inner_zip_keeping_its_own_name() {
        let b = bundle(&[
            ("graves v2.8.5.zip", b"one"),
            ("armor statues v2.8.21.zip", b"two"),
        ]);
        let out = split_bundle(&b).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "graves v2.8.5.zip");
        assert_eq!(out[0].1, b"one");
        assert_eq!(out[1].0, "armor statues v2.8.21.zip");
    }

    #[test]
    fn a_non_zip_entry_is_dropped_without_costing_its_siblings() {
        let b = bundle(&[("readme.txt", b"hi"), ("graves v2.8.5.zip", b"one")]);
        let out = split_bundle(&b).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "graves v2.8.5.zip");
    }

    #[test]
    fn a_bundle_that_is_not_a_zip_is_an_error_not_an_empty_list() {
        assert!(split_bundle(b"not a zip at all").is_err());
    }

    #[test]
    fn a_traversing_entry_name_is_reduced_to_its_file_name() {
        let b = bundle(&[("../../escape v1.0.zip", b"one")]);
        let out = split_bundle(&b).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "escape v1.0.zip");
    }
}

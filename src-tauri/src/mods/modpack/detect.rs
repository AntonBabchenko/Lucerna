//! Peek a zip and decide whether it is a Modrinth `.mrpack` or
//! a CurseForge modpack `.zip`.

use std::io::Cursor;

use crate::error::Error;
use crate::mods::modpack::schema::ModpackFormat;

pub fn detect_format(bytes: &[u8]) -> Result<ModpackFormat, Error> {
    let mut zip =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| Error::ModpackInvalidArchive {
            details: e.to_string(),
        })?;
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();
    if names.iter().any(|n| n == "modrinth.index.json") {
        Ok(ModpackFormat::Modrinth)
    } else if names.iter().any(|n| n == "manifest.json") {
        Ok(ModpackFormat::Curseforge)
    } else {
        Err(Error::ModpackFormatUnknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn make_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            for (name, body) in files {
                w.start_file(*name, SimpleFileOptions::default()).unwrap();
                w.write_all(body).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn detects_modrinth_when_index_json_present() {
        let zip = make_zip(&[("modrinth.index.json", br#"{}"#)]);
        assert_eq!(detect_format(&zip).unwrap(), ModpackFormat::Modrinth);
    }

    #[test]
    fn detects_curseforge_when_manifest_json_present() {
        let zip = make_zip(&[("manifest.json", br#"{}"#)]);
        assert_eq!(detect_format(&zip).unwrap(), ModpackFormat::Curseforge);
    }

    #[test]
    fn modrinth_wins_when_both_markers_present() {
        let zip = make_zip(&[
            ("modrinth.index.json", br#"{}"#),
            ("manifest.json", br#"{}"#),
        ]);
        assert_eq!(detect_format(&zip).unwrap(), ModpackFormat::Modrinth);
    }

    #[test]
    fn rejects_zip_without_known_marker() {
        let zip = make_zip(&[("README.md", b"hello")]);
        assert!(matches!(
            detect_format(&zip),
            Err(Error::ModpackFormatUnknown)
        ));
    }

    #[test]
    fn rejects_not_a_zip() {
        assert!(matches!(
            detect_format(b"not a zip"),
            Err(Error::ModpackInvalidArchive { .. })
        ));
    }
}

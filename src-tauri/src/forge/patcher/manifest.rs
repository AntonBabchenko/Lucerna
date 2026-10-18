//! The `Main-Class` of a processor jar, as Forge's installer reads it
//! (`JarFile.getManifest().getMainAttributes().getValue(MAIN_CLASS)`).

use std::io::Read;
use std::path::Path;

/// Parse a manifest's main section for `Main-Class`. Handles CRLF/LF/CR line
/// ends and 72-byte continuation lines (a line starting with one space
/// continues the previous one). The main section ends at the first empty line.
/// Attribute names compare case-insensitively, as Java's `Attributes.Name` does.
pub fn main_class_from_manifest(text: &str) -> Option<String> {
    let mut logical: Vec<String> = Vec::new();
    // `\r\n`, `\n` and `\r` all end a line. The split chain yields an empty
    // piece between `\r` and `\n` only for `\r\r\n`, which no jar tool writes.
    for raw in text
        .split("\r\n")
        .flat_map(|l| l.split('\n'))
        .flat_map(|l| l.split('\r'))
    {
        if raw.is_empty() {
            break; // end of the main section
        }
        if let Some(rest) = raw.strip_prefix(' ') {
            if let Some(last) = logical.last_mut() {
                last.push_str(rest);
            }
            continue;
        }
        logical.push(raw.to_string());
    }
    logical.iter().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if !name.trim().eq_ignore_ascii_case("Main-Class") {
            return None;
        }
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// Read `META-INF/MANIFEST.MF` from `jar` and return its `Main-Class`.
/// `Err` says why there is none: an unreadable jar, no manifest, or no
/// attribute.
pub fn jar_main_class(jar: &Path) -> Result<String, String> {
    let file = std::fs::File::open(jar).map_err(|e| format!("open {}: {e}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("read {}: {e}", jar.display()))?;
    let mut entry = zip
        .by_name("META-INF/MANIFEST.MF")
        .map_err(|_| format!("{} has no META-INF/MANIFEST.MF", jar.display()))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| format!("read the manifest of {}: {e}", jar.display()))?;
    main_class_from_manifest(&text)
        .ok_or_else(|| format!("{} does not have a Main-Class", jar.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_main_class_with_crlf() {
        let m = "Manifest-Version: 1.0\r\nMain-Class: net.minecraftforge.jarsplitter.ConsoleTool\r\n\r\n";
        assert_eq!(
            main_class_from_manifest(m).as_deref(),
            Some("net.minecraftforge.jarsplitter.ConsoleTool")
        );
    }

    #[test]
    fn joins_continuation_lines() {
        let m =
            "Manifest-Version: 1.0\nMain-Class: net.neoforged.installertools.Cons\n oleTool\n\n";
        assert_eq!(
            main_class_from_manifest(m).as_deref(),
            Some("net.neoforged.installertools.ConsoleTool")
        );
    }

    #[test]
    fn name_is_case_insensitive_and_section_ends_at_blank_line() {
        assert_eq!(
            main_class_from_manifest("main-class: a.B\n").as_deref(),
            Some("a.B")
        );
        let per_entry_only = "Manifest-Version: 1.0\n\nName: x\nMain-Class: not.Main\n";
        assert_eq!(main_class_from_manifest(per_entry_only), None);
    }

    #[test]
    fn empty_value_is_none() {
        assert_eq!(main_class_from_manifest("Main-Class: \n"), None);
    }

    #[test]
    fn jar_without_manifest_says_so() {
        use std::io::Write;
        let td = tempfile::tempdir().unwrap();
        let jar = td.path().join("x.jar");
        {
            let f = std::fs::File::create(&jar).unwrap();
            let mut z = zip::ZipWriter::new(f);
            z.start_file("a.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
        }
        let err = jar_main_class(&jar).unwrap_err();
        assert!(err.contains("MANIFEST"), "{err}");
    }

    #[test]
    fn jar_with_manifest_gives_its_main_class() {
        use std::io::Write;
        let td = tempfile::tempdir().unwrap();
        let jar = td.path().join("x.jar");
        {
            let f = std::fs::File::create(&jar).unwrap();
            let mut z = zip::ZipWriter::new(f);
            z.start_file(
                "META-INF/MANIFEST.MF",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            z.write_all(b"Manifest-Version: 1.0\r\nMain-Class: a.b.Main\r\n\r\n")
                .unwrap();
            z.finish().unwrap();
        }
        assert_eq!(jar_main_class(&jar).as_deref(), Ok("a.b.Main"));
    }
}

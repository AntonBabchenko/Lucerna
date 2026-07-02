//! Unzip lwjgl-style native classifier jars into `<instance>/natives/`
//! so Java can find `lwjgl.dll`, `OpenAL.dll`, etc. via
//! `-Djava.library.path=...`.
//!
//! Mojang publishes the natives inside per-platform classifier jars
//! (`...-natives-windows.jar`); we already downloaded them in slice 4.
//! This module reads each one and dumps its non-META-INF entries into
//! the target directory.

use crate::error::{Error, Result};
use crate::versions::libraries::should_install;
use crate::versions::version_json::Library;
use std::path::{Path, PathBuf};

/// Extract every platform-applicable native classifier jar into `dest`.
/// Overwrites existing files — cheap (~5 MB total) and avoids
/// stale-natives bugs after MC version changes.
pub async fn extract_natives(
    libs: &[Library],
    libraries_dir: &Path,
    dest: &Path,
    os: &str,
    arch: &str,
) -> Result<()> {
    tokio::fs::create_dir_all(dest)
        .await
        .map_err(|e| Error::io(dest.display().to_string(), e))?;

    let jars: Vec<PathBuf> = libs
        .iter()
        .filter(|lib| should_install(lib, os, arch))
        .filter_map(|lib| native_jar_path(lib, libraries_dir, os, arch))
        .collect();

    for jar_path in jars {
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || extract_one(&jar_path, &dest))
            .await
            .map_err(|e| Error::io("<spawn_blocking>".to_string(), e))??;
    }
    Ok(())
}

fn native_jar_path(lib: &Library, libraries_dir: &Path, os: &str, arch: &str) -> Option<PathBuf> {
    let natives_map = lib.natives.as_ref()?;
    let classifier_key = natives_map.get(os)?;
    let key = classifier_key.replace("${arch}", arch);
    let downloads = lib.downloads.as_ref()?;
    let classifiers = downloads.classifiers.as_ref()?;
    let artifact = classifiers.get(&key)?;
    Some(libraries_dir.join(&artifact.path))
}

/// Sync — runs in `spawn_blocking` because the `zip` crate is sync.
fn extract_one(jar_path: &Path, dest: &Path) -> Result<()> {
    let file =
        std::fs::File::open(jar_path).map_err(|e| Error::io(jar_path.display().to_string(), e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| Error::io(jar_path.display().to_string(), format!("zip open: {e}")))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| {
            Error::io(
                jar_path.display().to_string(),
                format!("zip entry {i}: {e}"),
            )
        })?;
        if entry.is_dir() {
            continue;
        }
        if entry.name().starts_with("META-INF/") {
            continue;
        }
        // Refuse zip slip. `enclosed_name()` returns `None` for any entry
        // whose path would escape the extraction root — `..` traversal AND
        // absolute/rooted names (`/etc/x`, `C:\x`, a `\\?\` prefix on
        // Windows). The earlier `contains("..")` check missed rooted names:
        // `dest.join("/abs/path")` discards `dest` entirely on Unix, so a
        // malicious jar could write outside the natives dir. Mojang's jars
        // are clean, but we parse zips from external sources and this is a
        // known CVE pattern.
        let Some(safe_rel) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(&safe_rel);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io(parent.display().to_string(), e))?;
        }
        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| Error::io(out_path.display().to_string(), e))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| Error::io(out_path.display().to_string(), e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions::version_json::{Artifact, Library, LibraryDownloads};
    use std::collections::HashMap;
    use std::io::Write;

    fn make_lib_with_natives(jar_filename: &str, classifier: &str) -> Library {
        let mut classifiers = HashMap::new();
        classifiers.insert(
            classifier.to_string(),
            Artifact {
                path: jar_filename.to_string(),
                url: "u".into(),
                sha1: "s".into(),
                size: 0,
            },
        );
        let mut natives = HashMap::new();
        natives.insert("windows".to_string(), classifier.to_string());
        Library {
            name: "org.lwjgl:lwjgl:3.3.1".into(),
            downloads: Some(LibraryDownloads {
                artifact: None,
                classifiers: Some(classifiers),
            }),
            url: None,
            rules: None,
            natives: Some(natives),
        }
    }

    #[tokio::test]
    async fn extracts_dll_skips_meta_inf() {
        let tmp = tempfile::tempdir().expect("tmp");
        let libs_dir = tmp.path().join("libraries");
        let dest = tmp.path().join("natives");
        std::fs::create_dir_all(&libs_dir).unwrap();

        // Build a tiny zip with one .dll, a META-INF entry, and a nested entry.
        let jar_path = libs_dir.join("test-natives.jar");
        {
            let f = std::fs::File::create(&jar_path).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file("lwjgl.dll", opts).unwrap();
            w.write_all(b"PE-binary-bytes-go-here").unwrap();
            w.start_file("META-INF/MANIFEST.MF", opts).unwrap();
            w.write_all(b"Manifest-Version: 1.0\n").unwrap();
            w.start_file("sub/deep/native.dll", opts).unwrap();
            w.write_all(b"deeper").unwrap();
            w.finish().unwrap();
        }

        let lib = make_lib_with_natives("test-natives.jar", "natives-windows");
        extract_natives(&[lib], &libs_dir, &dest, "windows", "x64")
            .await
            .expect("extract");

        assert!(dest.join("lwjgl.dll").exists(), "top-level dll extracted");
        assert!(
            dest.join("sub/deep/native.dll").exists(),
            "nested dll extracted"
        );
        assert!(
            !dest.join("META-INF/MANIFEST.MF").exists(),
            "META-INF must be skipped"
        );
    }

    #[tokio::test]
    async fn skips_zip_slip_and_rooted_entry_names() {
        let tmp = tempfile::tempdir().expect("tmp");
        let libs_dir = tmp.path().join("libraries");
        let dest = tmp.path().join("natives");
        std::fs::create_dir_all(&libs_dir).unwrap();

        // A jar with a legit dll plus a `..` traversal and an absolute-path
        // entry. Only the legit dll should land; the two escaping entries
        // must be skipped by the `enclosed_name()` guard.
        let jar_path = libs_dir.join("evil-natives.jar");
        {
            let f = std::fs::File::create(&jar_path).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            w.start_file("ok.dll", opts).unwrap();
            w.write_all(b"good").unwrap();
            w.start_file("../escape.dll", opts).unwrap();
            w.write_all(b"traversal").unwrap();
            // Absolute/rooted name: `dest.join("/abs.dll")` would discard
            // `dest` on Unix without the guard.
            w.start_file("/abs.dll", opts).unwrap();
            w.write_all(b"rooted").unwrap();
            w.finish().unwrap();
        }

        let lib = make_lib_with_natives("evil-natives.jar", "natives-windows");
        extract_natives(&[lib], &libs_dir, &dest, "windows", "x64")
            .await
            .expect("extract");

        assert!(dest.join("ok.dll").exists(), "legit dll extracted");
        // The traversal target would sit next to `natives/` (its parent).
        assert!(
            !tmp.path().join("escape.dll").exists(),
            "`..` traversal entry must be skipped"
        );
    }

    #[tokio::test]
    async fn no_natives_classifier_is_noop() {
        let tmp = tempfile::tempdir().expect("tmp");
        let libs_dir = tmp.path().join("libraries");
        let dest = tmp.path().join("natives");
        std::fs::create_dir_all(&libs_dir).unwrap();

        let lib = Library {
            name: "com.mojang:authlib:3.x".into(),
            downloads: None,
            url: None,
            rules: None,
            natives: None,
        };
        extract_natives(&[lib], &libs_dir, &dest, "windows", "x64")
            .await
            .expect("noop");

        assert!(dest.exists());
        let entries: Vec<_> = std::fs::read_dir(&dest).unwrap().collect();
        assert!(entries.is_empty(), "no files extracted");
    }
}

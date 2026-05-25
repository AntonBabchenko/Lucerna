//! Zip archive operations for world backups. `zip_dir` writes a
//! folder into a `.zip` file; `extract_zip` extracts a `.zip` into
//! a target folder with zip-slip defense (entries that escape the
//! target via `..` are rejected).

use crate::error::{Error, Result};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// Recursively zip `src_dir` into `dest_zip`. The zip's root contains
/// a single entry named `root_name` — i.e. extracting it produces a
/// folder named `root_name`. Existing `dest_zip` is overwritten.
///
/// Compression is Deflate (default for the `zip` crate with the
/// `deflate` feature). Acceptable balance of CPU and compression for
/// Minecraft NBT + region files.
pub fn zip_dir(src_dir: &Path, dest_zip: &Path, root_name: &str) -> Result<()> {
    let file = File::create(dest_zip).map_err(|e| Error::io(dest_zip.display().to_string(), e))?;
    let mut zw = ZipWriter::new(BufWriter::new(file));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    add_dir_contents(&mut zw, src_dir, &PathBuf::from(root_name), &options)?;
    zw.finish()
        .map_err(|e| Error::io(dest_zip.display().to_string(), format!("zip finish: {e}")))?;
    Ok(())
}

fn add_dir_contents<W: Write + std::io::Seek>(
    zw: &mut ZipWriter<W>,
    fs_dir: &Path,
    zip_prefix: &Path,
    options: &SimpleFileOptions,
) -> Result<()> {
    // Add the directory entry itself so empty dirs are preserved on
    // extract (Minecraft's saves/ tree contains empty stub dirs in
    // some versions; not strictly required but cheap).
    let dir_entry = format!("{}/", zip_prefix.to_string_lossy().replace('\\', "/"));
    zw.add_directory(&dir_entry, *options)
        .map_err(|e| Error::io(fs_dir.display().to_string(), format!("zip dir: {e}")))?;
    for entry in
        std::fs::read_dir(fs_dir).map_err(|e| Error::io(fs_dir.display().to_string(), e))?
    {
        let entry = entry.map_err(|e| Error::io(fs_dir.display().to_string(), e))?;
        let path = entry.path();
        let name = entry.file_name();
        let zip_path = zip_prefix.join(&name);
        let meta = entry
            .metadata()
            .map_err(|e| Error::io(path.display().to_string(), e))?;
        if meta.is_dir() {
            add_dir_contents(zw, &path, &zip_path, options)?;
        } else if meta.is_file() {
            let zip_name = zip_path.to_string_lossy().replace('\\', "/");
            zw.start_file(&zip_name, *options)
                .map_err(|e| Error::io(path.display().to_string(), format!("zip start: {e}")))?;
            let mut f = BufReader::new(
                File::open(&path).map_err(|e| Error::io(path.display().to_string(), e))?,
            );
            std::io::copy(&mut f, zw).map_err(|e| Error::io(path.display().to_string(), e))?;
        }
        // symlinks/special files: skipped (defensive; saves don't have them)
    }
    Ok(())
}

/// Extract `src_zip` into `dest_dir`. Refuses any entry whose path
/// (after normalization) tries to escape `dest_dir` via `..` or an
/// absolute root — the classic zip-slip vulnerability. Refused
/// entries surface as `Error::BackupCorrupt` with a message naming
/// the bad entry; partial extracts are NOT cleaned up here (caller
/// responsibility — see restore.rs's tmp-dir flow).
pub fn extract_zip(src_zip: &Path, dest_dir: &Path) -> Result<()> {
    let file = File::open(src_zip).map_err(|e| Error::io(src_zip.display().to_string(), e))?;
    let mut archive = ZipArchive::new(BufReader::new(file)).map_err(|e| Error::BackupCorrupt {
        filename: src_zip
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .into(),
        details: format!("open: {e}"),
    })?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| Error::BackupCorrupt {
            filename: src_zip
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .into(),
            details: format!("entry {i}: {e}"),
        })?;
        let raw_name = entry.name().to_string();
        // Zip-slip defense: reject absolute paths, drive letters, and
        // any `..` segment. We do this BEFORE join so a `dest_dir
        // .join("../escape")` never gets canonicalized.
        if raw_name.starts_with('/')
            || raw_name.starts_with('\\')
            || raw_name.contains(':')
            || raw_name.split(['/', '\\']).any(|seg| seg == "..")
        {
            return Err(Error::BackupCorrupt {
                filename: src_zip
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .into(),
                details: format!("unsafe path: {raw_name}"),
            });
        }
        let dest_path = dest_dir.join(&raw_name);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest_path)
                .map_err(|e| Error::io(dest_path.display().to_string(), e))?;
            continue;
        }
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io(parent.display().to_string(), e))?;
        }
        let mut out = BufWriter::new(
            File::create(&dest_path).map_err(|e| Error::io(dest_path.display().to_string(), e))?,
        );
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = entry.read(&mut buf).map_err(|e| Error::BackupCorrupt {
                filename: src_zip
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .into(),
                details: format!("read {raw_name}: {e}"),
            })?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])
                .map_err(|e| Error::io(dest_path.display().to_string(), e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn read_file(p: &Path) -> Vec<u8> {
        fs::read(p).unwrap()
    }

    #[test]
    fn zip_dir_round_trip_byte_identical() {
        let src = tempdir().unwrap();
        let world = src.path().join("WORLD");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("level.dat"), b"\x01\x02\x03").unwrap();
        let region = world.join("region");
        fs::create_dir_all(&region).unwrap();
        fs::write(region.join("r.0.0.mca"), vec![0xAB; 1024]).unwrap();

        let zip_dir_td = tempdir().unwrap();
        let zip_path = zip_dir_td.path().join("backup.zip");
        zip_dir(&world, &zip_path, "WORLD").unwrap();

        let out = tempdir().unwrap();
        extract_zip(&zip_path, out.path()).unwrap();

        // Extract produces out/WORLD/{level.dat, region/r.0.0.mca}
        let extracted = out.path().join("WORLD");
        assert!(extracted.is_dir());
        assert_eq!(read_file(&extracted.join("level.dat")), b"\x01\x02\x03");
        assert_eq!(
            read_file(&extracted.join("region").join("r.0.0.mca")),
            vec![0xAB; 1024]
        );
    }

    #[test]
    fn zip_dir_preserves_nested_subdirs() {
        let src = tempdir().unwrap();
        let world = src.path().join("W");
        let deep = world.join("a").join("b").join("c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.txt"), b"deep").unwrap();

        let zip_td = tempdir().unwrap();
        let zip_path = zip_td.path().join("z.zip");
        zip_dir(&world, &zip_path, "W").unwrap();

        let out = tempdir().unwrap();
        extract_zip(&zip_path, out.path()).unwrap();
        assert_eq!(
            read_file(
                &out.path()
                    .join("W")
                    .join("a")
                    .join("b")
                    .join("c")
                    .join("x.txt")
            ),
            b"deep"
        );
    }

    #[test]
    fn extract_zip_rejects_path_traversal() {
        // Handcraft a zip whose single entry is "../escape.txt".
        let zip_td = tempdir().unwrap();
        let zip_path = zip_td.path().join("evil.zip");
        {
            let file = File::create(&zip_path).unwrap();
            let mut zw = ZipWriter::new(BufWriter::new(file));
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("../escape.txt", options).unwrap();
            zw.write_all(b"pwned").unwrap();
            zw.finish().unwrap();
        }
        let out = tempdir().unwrap();
        let r = extract_zip(&zip_path, out.path());
        assert!(
            matches!(r, Err(Error::BackupCorrupt { .. })),
            "expected BackupCorrupt for zip-slip attempt, got: {r:?}"
        );
        // And the escape file MUST NOT exist outside.
        assert!(!out.path().parent().unwrap().join("escape.txt").exists());
    }

    #[test]
    fn extract_zip_rejects_absolute_path() {
        let zip_td = tempdir().unwrap();
        let zip_path = zip_td.path().join("evil2.zip");
        {
            let file = File::create(&zip_path).unwrap();
            let mut zw = ZipWriter::new(BufWriter::new(file));
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            // Note: zip-spec entry paths use forward slash. Leading slash
            // would make this an absolute-path attack.
            zw.start_file("/etc/passwd", options).unwrap();
            zw.write_all(b"pwned").unwrap();
            zw.finish().unwrap();
        }
        let out = tempdir().unwrap();
        assert!(matches!(
            extract_zip(&zip_path, out.path()),
            Err(Error::BackupCorrupt { .. })
        ));
    }

    #[test]
    fn extract_zip_handles_empty_dir_entries() {
        let src = tempdir().unwrap();
        let world = src.path().join("E");
        let empty_sub = world.join("empty");
        fs::create_dir_all(&empty_sub).unwrap();

        let zip_td = tempdir().unwrap();
        let zip_path = zip_td.path().join("e.zip");
        zip_dir(&world, &zip_path, "E").unwrap();

        let out = tempdir().unwrap();
        extract_zip(&zip_path, out.path()).unwrap();
        assert!(out.path().join("E").join("empty").is_dir());
    }

    #[test]
    fn extract_zip_returns_corrupt_for_truncated_zip() {
        let zip_td = tempdir().unwrap();
        let zip_path = zip_td.path().join("trunc.zip");
        // Not a valid zip at all — just garbage bytes.
        fs::write(&zip_path, b"not a zip file").unwrap();
        let out = tempdir().unwrap();
        let r = extract_zip(&zip_path, out.path());
        assert!(matches!(r, Err(Error::BackupCorrupt { .. })), "got: {r:?}");
    }
}

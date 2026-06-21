//! Zip assembly + overrides.

use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

use sha1::Sha1;
use sha2::{Digest as Sha2Digest, Sha512};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::error::Error;
use crate::mods::modpack::path_safety::is_safe_relative_path;
use crate::mods::platform::ModSource;

/// Lowercase hex sha1 + sha512 and byte size of the file at `path`.
/// Reads the whole file once. Used to build `.mrpack` file hashes from the
/// local jar (the source of truth — we do not trust the registry's sha1).
pub fn hash_file(path: &Path) -> Result<(String, String, u64), Error> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path.display().to_string(), e))?;
    // sha1 0.11 and sha2 0.10 pull in different versions of the `digest`
    // crate; use fully-qualified trait calls to avoid the ambiguity.
    let sha1 = {
        use sha1::Digest as Sha1Digest;
        hex::encode(Sha1::digest(&bytes))
    };
    let sha512 = hex::encode(Sha512::digest(&bytes));
    Ok((sha1, sha512, bytes.len() as u64))
}

/// Resolve the canonical download URL for a referenced mod. Modrinth: the
/// primary file URL from the version endpoint. CurseForge: the forgecdn
/// download URL (also an allowed mrpack download host). Returns
/// `Ok(None)` when the platform refuses distribution (CF disabled) — the
/// caller falls back to bundling the local jar.
pub async fn resolve_download_url(
    source: ModSource,
    project_id: &str,
    version_id: &str,
) -> Result<Option<String>, Error> {
    match source {
        ModSource::Modrinth => {
            let url =
                format!("https://api.modrinth.com/v2/project/{project_id}/version/{version_id}");
            let resp = crate::network::request::get(
                &url,
                &[("user-agent", "AntonBabchenko/Lucerna")],
                "modpacks",
            )
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
            if !(200..300).contains(&resp.status) {
                return Err(Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            #[derive(serde::Deserialize)]
            struct V {
                files: Vec<F>,
            }
            #[derive(serde::Deserialize)]
            struct F {
                url: String,
                primary: bool,
            }
            let v: V = serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                platform: "modrinth".into(),
                details: e.to_string(),
            })?;
            Ok(v.files
                .iter()
                .find(|f| f.primary)
                .or_else(|| v.files.first())
                .map(|f| f.url.clone()))
        }
        ModSource::Curseforge => {
            let key = crate::mods::curseforge::keyring::resolve();
            match crate::mods::modpack::cf_api::resolve_file_download(
                "https://api.curseforge.com",
                key.as_deref(),
                project_id,
                version_id,
            )
            .await
            {
                Ok(url) => Ok(Some(url)),
                // Distribution disabled → caller bundles the local jar instead.
                Err(Error::ModpackCfDistributionDisabled { .. }) => Ok(None),
                Err(e) => Err(e),
            }
        }
        // FTB: pack-managed mods have no canonical download URL for export; bundle locally.
        ModSource::Ftb => Ok(None),
        // ATLauncher: pack-managed mods have no canonical download URL for export; bundle locally.
        ModSource::Atlauncher => Ok(None),
    }
}

/// A file to place inside the archive. `archive_path` is the full path
/// within the zip using forward slashes (e.g. `overrides/mods/x.jar` or
/// `modrinth.index.json`).
pub struct ZipEntry {
    pub archive_path: String,
    pub source: ZipSource,
}

pub enum ZipSource {
    /// Copy this on-disk file into the entry.
    File(std::path::PathBuf),
    /// Write these in-memory bytes (the manifest JSON).
    Bytes(Vec<u8>),
}

/// Write `entries` into a new zip at `dest`. Overwrites any existing file.
/// Every `archive_path` under `overrides/` must be a safe relative path
/// after the prefix; rejects traversal.
pub fn write_archive(dest: &Path, entries: &[ZipEntry]) -> Result<(), Error> {
    let file = std::fs::File::create(dest).map_err(|e| Error::io(dest.display().to_string(), e))?;
    let mut zw = ZipWriter::new(BufWriter::new(file));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for entry in entries {
        // Guard: anything under overrides/ must be a safe relative path.
        if let Some(rel) = entry.archive_path.strip_prefix("overrides/") {
            if !is_safe_relative_path(rel) {
                return Err(Error::ModpackExportFailed {
                    details: format!("unsafe override path: {}", entry.archive_path),
                });
            }
        }
        zw.start_file(&entry.archive_path, options)
            .map_err(|e| Error::ModpackExportFailed {
                details: format!("zip start {}: {e}", entry.archive_path),
            })?;
        match &entry.source {
            ZipSource::Bytes(b) => {
                zw.write_all(b)
                    .map_err(|e| Error::io(entry.archive_path.clone(), e))?;
            }
            ZipSource::File(p) => {
                let mut f = BufReader::new(
                    std::fs::File::open(p).map_err(|e| Error::io(p.display().to_string(), e))?,
                );
                std::io::copy(&mut f, &mut zw)
                    .map_err(|e| Error::io(p.display().to_string(), e))?;
            }
        }
    }
    zw.finish().map_err(|e| Error::ModpackExportFailed {
        details: format!("zip finish: {e}"),
    })?;
    Ok(())
}

/// Recursively collect files under `dir` into `ZipEntry`s rooted at
/// `archive_prefix` (e.g. `overrides/config`). Returns empty when `dir`
/// does not exist. Files are added; directories are recursed.
/// **Symlinks are never followed** — any symlink entry is silently skipped
/// to prevent infinite cycles and to avoid leaking external files into the
/// shared archive.
pub fn collect_dir_entries(dir: &Path, archive_prefix: &str) -> Result<Vec<ZipEntry>, Error> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    collect_recursive(dir, archive_prefix, &mut out)?;
    Ok(out)
}

fn collect_recursive(dir: &Path, prefix: &str, out: &mut Vec<ZipEntry>) -> Result<(), Error> {
    for entry in std::fs::read_dir(dir).map_err(|e| Error::io(dir.display().to_string(), e))? {
        let entry = entry.map_err(|e| Error::io(dir.display().to_string(), e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let child_prefix = format!("{prefix}/{name}");
        // Use file_type() (does NOT follow symlinks — lstat semantics) to
        // detect and skip symlinks. This prevents: (a) infinite recursion from
        // symlink cycles, (b) external files leaking into the shared archive.
        let ft = entry
            .file_type()
            .map_err(|e| Error::io(path.display().to_string(), e))?;
        if ft.is_symlink() {
            continue; // never follow symlinks: avoids cycles + prevents leaking external files
        }
        if ft.is_dir() {
            collect_recursive(&path, &child_prefix, out)?;
        } else if ft.is_file() {
            out.push(ZipEntry {
                archive_path: child_prefix,
                source: ZipSource::File(path),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod archive_tests {
    use super::*;
    use std::io::Read;
    use tempfile::tempdir;
    use zip::ZipArchive;

    #[test]
    fn writes_manifest_and_override_file() {
        let td = tempdir().unwrap();
        let jar = td.path().join("a.jar");
        std::fs::write(&jar, b"jarbytes").unwrap();
        let dest = td.path().join("out.mrpack");
        let entries = vec![
            ZipEntry {
                archive_path: "modrinth.index.json".into(),
                source: ZipSource::Bytes(b"{}".to_vec()),
            },
            ZipEntry {
                archive_path: "overrides/mods/a.jar".into(),
                source: ZipSource::File(jar),
            },
        ];
        write_archive(&dest, &entries).unwrap();

        let mut zip = ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
        let mut idx = String::new();
        zip.by_name("modrinth.index.json")
            .unwrap()
            .read_to_string(&mut idx)
            .unwrap();
        assert_eq!(idx, "{}");
        let mut body = Vec::new();
        zip.by_name("overrides/mods/a.jar")
            .unwrap()
            .read_to_end(&mut body)
            .unwrap();
        assert_eq!(body, b"jarbytes");
    }

    #[test]
    fn rejects_unsafe_override_path() {
        let td = tempdir().unwrap();
        let dest = td.path().join("out.mrpack");
        let entries = vec![ZipEntry {
            archive_path: "overrides/../escape.txt".into(),
            source: ZipSource::Bytes(b"x".to_vec()),
        }];
        let r = write_archive(&dest, &entries);
        assert!(matches!(r, Err(Error::ModpackExportFailed { .. })));
    }

    #[test]
    fn collect_dir_entries_walks_recursively() {
        let td = tempdir().unwrap();
        let cfg = td.path().join("config").join("sodium");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("options.json"), b"{}").unwrap();
        let entries = collect_dir_entries(&td.path().join("config"), "overrides/config").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].archive_path,
            "overrides/config/sodium/options.json"
        );
    }

    #[test]
    fn collect_dir_entries_empty_for_missing_dir() {
        let td = tempdir().unwrap();
        let entries = collect_dir_entries(&td.path().join("nope"), "overrides/nope").unwrap();
        assert!(entries.is_empty());
    }

    /// Verify that a nested directory tree (real files only, no symlinks) is
    /// walked correctly — cross-platform safe.
    #[test]
    fn collect_dir_entries_walks_nested_dirs() {
        let td = tempdir().unwrap();
        let deep = td.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("leaf.txt"), b"data").unwrap();
        let entries = collect_dir_entries(&td.path().join("a"), "overrides/a").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].archive_path, "overrides/a/b/c/leaf.txt");
    }

    /// On Unix, a symlink inside a scanned directory must be silently skipped.
    /// The test is Unix-only because creating symlinks on Windows requires
    /// elevated privileges that may not be available in CI.
    #[test]
    #[cfg(unix)]
    fn collect_dir_entries_skips_symlinks() {
        use std::os::unix::fs::symlink;
        let td = tempdir().unwrap();
        let dir = td.path().join("content");
        std::fs::create_dir_all(&dir).unwrap();
        // A real file that must be included.
        std::fs::write(dir.join("real.txt"), b"hi").unwrap();
        // A symlink that must be skipped (target doesn't even need to exist).
        symlink("/etc/hosts", dir.join("sym.txt")).unwrap();
        // A symlink to a directory — also must be skipped (not recursed).
        symlink("/tmp", dir.join("symdir")).unwrap();
        let entries = collect_dir_entries(&dir, "overrides/content").unwrap();
        // Only the real file should appear.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].archive_path, "overrides/content/real.txt");
    }
}

#[cfg(test)]
mod hash_tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn hashes_known_bytes() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"abc").unwrap();
        let (sha1, sha512, size) = hash_file(f.path()).unwrap();
        // Known digests of "abc".
        assert_eq!(sha1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha512,
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(size, 3);
    }
}

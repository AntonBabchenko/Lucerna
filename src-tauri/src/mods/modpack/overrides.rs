//! Extract `overrides/` and (Modrinth-only) `client-overrides/` from a
//! modpack zip into `{instance_root}/.minecraft/`. Zip-slip-safe.

use std::io::{Cursor, Read};
use std::path::{Component, Path};

use sha1::{Digest, Sha1};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::Error;

const PER_FILE_CAP: u64 = 200 * 1024 * 1024;
const AGGREGATE_CAP: u64 = 2 * 1024 * 1024 * 1024;

/// What we keep about a zip entry once we've finished inspecting it.
/// Owns its bytes (or nothing, for a directory), so we can drop the
/// underlying non-`Send` `ZipFile<'_>` before doing async IO.
enum EntryKind {
    Dir,
    File(Vec<u8>),
}

/// A `.jar` file the extractor placed into `mods/`. The orchestrator
/// (`modpack::import::import`) folds these into `pack_origin.files` as
/// bundled entries so the drawer can badge them as "pack" instead of
/// "manual". `url` stays empty because the bytes came from inside the
/// `.mrpack` archive, not a network source — Restore is not possible
/// for these without re-importing the archive.
#[derive(Debug, Clone)]
pub struct ExtractedMod {
    pub install_path: String,
    pub filename: String,
    pub sha1: String,
    pub size: u64,
}

pub async fn extract<F: FnMut(u32, u32)>(
    bytes: &[u8],
    instance_root: &Path,
    mut on_progress: F,
) -> Result<Vec<ExtractedMod>, Error> {
    let mc_dir = instance_root.join(".minecraft");
    fs::create_dir_all(&mc_dir).await.map_err(|e| Error::Io {
        path: mc_dir.display().to_string(),
        details: e.to_string(),
    })?;
    let mc_dir_canon = dunce::canonicalize(&mc_dir).map_err(|e| Error::Io {
        path: mc_dir.display().to_string(),
        details: e.to_string(),
    })?;

    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| Error::ModpackInvalidArchive { details: e.to_string() })?;

    let mut work: Vec<(usize, String)> = vec![];
    let mut client_paths: std::collections::HashSet<String> = Default::default();
    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|e| Error::ModpackInvalidArchive { details: e.to_string() })?;
        let name = entry.name().to_string();
        if let Some(rel) = name.strip_prefix("client-overrides/") {
            if !rel.is_empty() {
                client_paths.insert(rel.to_string());
                work.push((i, rel.to_string()));
            }
        }
    }
    // overrides/ comes second so client-overrides/ wins on conflict.
    let mut overrides_work: Vec<(usize, String)> = vec![];
    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|e| Error::ModpackInvalidArchive { details: e.to_string() })?;
        let name = entry.name().to_string();
        if let Some(rel) = name.strip_prefix("overrides/") {
            if !rel.is_empty() && !client_paths.contains(rel) {
                overrides_work.push((i, rel.to_string()));
            }
        }
    }
    work.extend(overrides_work);

    let total = work.len() as u32;
    on_progress(0, total);
    let mut aggregate: u64 = 0;
    let mut extracted_mods: Vec<ExtractedMod> = vec![];

    for (idx, (zip_idx, rel)) in work.into_iter().enumerate() {
        // Pull everything we need out of the `ZipFile` (non-`Send`,
        // borrows from `zip`) into owned values, then drop it BEFORE
        // any `.await`. The tokio fs/io ops below can therefore run
        // inside a `Send` future — needed by the Tauri command boundary.
        let kind: EntryKind = {
            let mut entry = zip
                .by_index(zip_idx)
                .map_err(|e| Error::ModpackInvalidArchive { details: e.to_string() })?;

            // Reject symlinks.
            if let Some(mode) = entry.unix_mode() {
                if mode & 0o170000 == 0o120000 {
                    return Err(Error::ModpackOverridesPathEscape { entry: rel });
                }
            }

            // Reject path traversal / abs / drive letter / backslash.
            for c in Path::new(&rel).components() {
                match c {
                    Component::Normal(_) => {}
                    _ => return Err(Error::ModpackOverridesPathEscape { entry: rel }),
                }
            }
            if rel.contains('\\')
                || rel.starts_with('/')
                || (rel.len() > 1 && rel.as_bytes()[1] == b':')
            {
                return Err(Error::ModpackOverridesPathEscape { entry: rel });
            }

            if entry.is_dir() {
                EntryKind::Dir
            } else {
                let size = entry.size();
                if size > PER_FILE_CAP {
                    return Err(Error::ModpackOverridesTooLarge {
                        entry: rel,
                        size: size as f64,
                        cap: PER_FILE_CAP as f64,
                    });
                }
                aggregate = aggregate.saturating_add(size);
                if aggregate > AGGREGATE_CAP {
                    return Err(Error::ModpackOverridesTooLarge {
                        entry: "<aggregate>".into(),
                        size: aggregate as f64,
                        cap: AGGREGATE_CAP as f64,
                    });
                }
                let mut buf = Vec::with_capacity(size as usize);
                entry.read_to_end(&mut buf).map_err(|e| Error::Io {
                    path: rel.clone(),
                    details: e.to_string(),
                })?;
                EntryKind::File(buf)
            }
            // `entry` (ZipFile<'_>) goes out of scope here — no `.await`
            // beyond this point holds it.
        };

        let target = mc_dir.join(&rel);
        match kind {
            EntryKind::Dir => {
                fs::create_dir_all(&target).await.map_err(|e| Error::Io {
                    path: target.display().to_string(),
                    details: e.to_string(),
                })?;
            }
            EntryKind::File(buf) => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).await.map_err(|e| Error::Io {
                        path: parent.display().to_string(),
                        details: e.to_string(),
                    })?;
                    let parent_canon = dunce::canonicalize(parent).map_err(|e| Error::Io {
                        path: parent.display().to_string(),
                        details: e.to_string(),
                    })?;
                    if !parent_canon.starts_with(&mc_dir_canon) {
                        return Err(Error::ModpackOverridesPathEscape { entry: rel });
                    }
                }
                let mut f = fs::File::create(&target).await.map_err(|e| Error::Io {
                    path: target.display().to_string(),
                    details: e.to_string(),
                })?;
                f.write_all(&buf).await.map_err(|e| Error::Io {
                    path: target.display().to_string(),
                    details: e.to_string(),
                })?;

                // If this jar landed under `mods/`, surface it to the
                // orchestrator so it can register the file in
                // pack_origin as a bundled entry. Without this the
                // drawer would later badge it "manual" — scan-reconcile
                // picks the orphan up with source=None and we'd lose
                // the pack-provenance signal.
                if (rel.starts_with("mods/") || rel.starts_with("mods\\"))
                    && (rel.ends_with(".jar") || rel.ends_with(".jar.disabled"))
                {
                    let filename = rel.rsplit(['/', '\\']).next().unwrap_or(&rel).to_string();
                    let sha1 = hex::encode(Sha1::digest(&buf));
                    extracted_mods.push(ExtractedMod {
                        install_path: rel.clone(),
                        filename,
                        sha1,
                        size: buf.len() as u64,
                    });
                }
            }
        }
        on_progress(idx as u32 + 1, total);
    }
    Ok(extracted_mods)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;
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

    #[tokio::test]
    async fn extracts_normal_file() {
        let zip = make_zip(&[("overrides/config/foo.toml", b"k=v")]);
        let inst = TempDir::new().unwrap();
        extract(&zip, inst.path(), |_, _| {}).await.unwrap();
        let target = inst.path().join(".minecraft/config/foo.toml");
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"k=v");
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let zip = make_zip(&[("overrides/../escape.txt", b"!")]);
        let inst = TempDir::new().unwrap();
        let r = extract(&zip, inst.path(), |_, _| {}).await;
        assert!(matches!(r, Err(Error::ModpackOverridesPathEscape { .. })));
    }

    #[tokio::test]
    async fn rejects_absolute_path() {
        let zip = make_zip(&[("overrides//etc/passwd", b"!")]);
        let inst = TempDir::new().unwrap();
        let r = extract(&zip, inst.path(), |_, _| {}).await;
        assert!(matches!(r, Err(Error::ModpackOverridesPathEscape { .. })));
    }

    #[tokio::test]
    async fn rejects_drive_letter() {
        let zip = make_zip(&[("overrides/C:/windows/system32.txt", b"!")]);
        let inst = TempDir::new().unwrap();
        let r = extract(&zip, inst.path(), |_, _| {}).await;
        assert!(matches!(r, Err(Error::ModpackOverridesPathEscape { .. })));
    }

    #[tokio::test]
    async fn client_overrides_win_on_conflict() {
        let zip = make_zip(&[
            ("overrides/options.txt", b"from-overrides"),
            ("client-overrides/options.txt", b"from-client"),
        ]);
        let inst = TempDir::new().unwrap();
        extract(&zip, inst.path(), |_, _| {}).await.unwrap();
        let bytes = tokio::fs::read(inst.path().join(".minecraft/options.txt"))
            .await
            .unwrap();
        assert_eq!(bytes, b"from-client");
    }

    #[tokio::test]
    async fn progress_callback_fires_total_then_each() {
        let zip = make_zip(&[("overrides/a.txt", b"a"), ("overrides/b.txt", b"b")]);
        let inst = TempDir::new().unwrap();
        let calls = std::sync::Mutex::new(vec![]);
        extract(&zip, inst.path(), |c, t| calls.lock().unwrap().push((c, t)))
            .await
            .unwrap();
        let calls = calls.into_inner().unwrap();
        assert_eq!(calls[0], (0, 2));
        assert!(calls.iter().any(|p| *p == (1, 2)));
        assert!(calls.iter().any(|p| *p == (2, 2)));
    }

    // Symlink-mode entries (`mode & 0o170000 == 0o120000`) and aggregate
    // cap (`> 2 GiB`) branches are exercised in production by real
    // adversarial zips (created with Unix `zip`/`7z`) and by huge packs
    // respectively. They are not unit-tested here because:
    //   - `zip` 2.x's `SimpleFileOptions::unix_permissions` masks to `0o777`
    //     so the file-type bits (S_IFLNK) cannot be set via the safe API.
    //     Coverage would require hand-crafting raw central-directory bytes,
    //     which is out of scope for v1.
    //   - The aggregate cap test would need 11+ × 200 MiB entries (~80 s
    //     wall clock) without refactoring the caps to be parameters. Per-
    //     file cap is exercised below; the aggregate code path is a copy of
    //     the same pattern with `aggregate.saturating_add`.

    #[tokio::test]
    async fn returns_extracted_mods_for_overrides_mods_jars() {
        // Bundled-mod tracking: when overrides contains .jar files under
        // mods/, extract() returns them so the orchestrator can register
        // them in pack_origin as bundled entries. .txt and non-mods/
        // paths are NOT returned.
        let zip = make_zip(&[
            ("overrides/mods/foo.jar", b"foo-bytes" as &[u8]),
            ("overrides/mods/bar.jar", b"bar-bytes" as &[u8]),
            ("overrides/mods/notes.txt", b"not-a-jar" as &[u8]),
            ("overrides/config/foo.toml", b"k=v" as &[u8]),
        ]);
        let inst = TempDir::new().unwrap();
        let extracted = extract(&zip, inst.path(), |_, _| {}).await.unwrap();
        assert_eq!(
            extracted.len(),
            2,
            "expected 2 .jar bundled mods, got {extracted:?}"
        );
        let foo = extracted.iter().find(|m| m.filename == "foo.jar").unwrap();
        let bar = extracted.iter().find(|m| m.filename == "bar.jar").unwrap();
        assert_eq!(foo.install_path, "mods/foo.jar");
        assert_eq!(foo.size, 9);
        assert_eq!(bar.install_path, "mods/bar.jar");
        // SHA-1 is computed from the actual bytes — sanity-check by
        // re-computing here.
        let expected_foo_sha = hex::encode(Sha1::digest(b"foo-bytes"));
        assert_eq!(foo.sha1, expected_foo_sha);
    }

    #[tokio::test]
    async fn per_file_cap_rejected() {
        // synthesize a zip entry claiming 201 MiB
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            w.start_file("overrides/huge.bin", SimpleFileOptions::default())
                .unwrap();
            let block = vec![0u8; 1024 * 1024];
            for _ in 0..201 {
                w.write_all(&block).unwrap();
            }
            w.finish().unwrap();
        }
        let inst = TempDir::new().unwrap();
        let r = extract(&buf, inst.path(), |_, _| {}).await;
        assert!(
            matches!(r, Err(Error::ModpackOverridesTooLarge { .. })),
            "got: {r:?}"
        );
    }
}

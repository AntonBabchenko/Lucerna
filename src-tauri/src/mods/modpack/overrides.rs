//! Extract `overrides/` and (Modrinth-only) `client-overrides/` from a
//! modpack zip into `{instance_root}/.minecraft/`. Zip-slip-safe.

use std::io::{Cursor, Read};
use std::path::Path;

use sha1::{Digest, Sha1};
use tokio::fs;

use crate::error::Error;
use crate::mods::modpack::schema::SkippedOverride;

const PER_FILE_CAP: u64 = 200 * 1024 * 1024;
const AGGREGATE_CAP: u64 = 2 * 1024 * 1024 * 1024;

/// What `extract` produced: the bundled assets it wrote to disk, plus any
/// `overrides/` entries it deliberately skipped for exceeding the per-file
/// cap. Skipping (rather than aborting the whole import) keeps a single
/// oversized non-mod blob — e.g. a `.rar` an author left in `mods/`, which
/// Minecraft can't load — from killing an otherwise-valid pack install.
#[derive(Debug, Default)]
pub struct ExtractOutcome {
    pub extracted: Vec<ExtractedAsset>,
    pub skipped: Vec<SkippedOverride>,
}

/// What we keep about a zip entry once we've finished inspecting it.
/// Owns its bytes (or nothing, for a directory), so we can drop the
/// underlying non-`Send` `ZipFile<'_>` before doing async IO.
enum EntryKind {
    Dir,
    File(Vec<u8>),
    /// Over the per-file cap — recorded as skipped and NOT read into
    /// memory (so a zip-bomb's declared size never forces an allocation).
    Oversized(u64),
}

/// A file the extractor placed under a tracked directory (`mods/`,
/// `resourcepacks/`, `shaderpacks/`). The orchestrator folds these into
/// `pack_origin.files` as bundled entries so the drawer badges them
/// "pack". `url` stays empty — the bytes came from inside the archive,
/// so Restore is not possible without re-importing.
#[derive(Debug, Clone)]
pub struct ExtractedAsset {
    pub install_path: String,
    pub filename: String,
    pub sha1: String,
    pub size: u64,
}

/// Which extracted files the orchestrator should record in pack_origin.
/// Bundled jars under `mods/`, plus top-level (single-segment) files
/// directly under `resourcepacks/` / `shaderpacks/` that are actually
/// loadable packs (`.zip` / `.zip.disabled`). A non-`.zip` file in those
/// dirs (e.g. a `.rar`/`.7z`/`.txt` download note an author bundled) is
/// extracted to disk but not itemised as that asset type. Folder-form
/// resourcepacks and bulk config trees are intentionally not itemised.
fn is_tracked_bundled_path(rel: &str) -> bool {
    if rel.starts_with("mods/") && (rel.ends_with(".jar") || rel.ends_with(".jar.disabled")) {
        return true;
    }
    let is_zip_pack = rel.ends_with(".zip") || rel.ends_with(".zip.disabled");
    for prefix in ["resourcepacks/", "shaderpacks/"] {
        if let Some(after) = rel.strip_prefix(prefix) {
            if !after.is_empty() && !after.contains('/') && is_zip_pack {
                return true;
            }
        }
    }
    false
}

pub async fn extract<F: FnMut(u32, u32)>(
    bytes: &[u8],
    instance_root: &Path,
    mut on_progress: F,
) -> Result<ExtractOutcome, Error> {
    let mc_dir = instance_root.join(".minecraft");
    fs::create_dir_all(&mc_dir).await.map_err(|e| Error::Io {
        path: mc_dir.display().to_string(),
        details: e.to_string(),
    })?;
    let mc_dir_canon = dunce::canonicalize(&mc_dir).map_err(|e| Error::Io {
        path: mc_dir.display().to_string(),
        details: e.to_string(),
    })?;

    let mut zip =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| Error::ModpackInvalidArchive {
            details: e.to_string(),
        })?;

    let mut work: Vec<(usize, String)> = vec![];
    let mut client_paths: std::collections::HashSet<String> = Default::default();
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| Error::ModpackInvalidArchive {
            details: e.to_string(),
        })?;
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
        let entry = zip.by_index(i).map_err(|e| Error::ModpackInvalidArchive {
            details: e.to_string(),
        })?;
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
    let mut extracted: Vec<ExtractedAsset> = vec![];
    let mut skipped: Vec<SkippedOverride> = vec![];

    for (idx, (zip_idx, rel)) in work.into_iter().enumerate() {
        // Pull everything we need out of the `ZipFile` (non-`Send`,
        // borrows from `zip`) into owned values, then drop it BEFORE
        // any `.await`. The tokio fs/io ops below can therefore run
        // inside a `Send` future — needed by the Tauri command boundary.
        let kind: EntryKind = {
            let mut entry = zip
                .by_index(zip_idx)
                .map_err(|e| Error::ModpackInvalidArchive {
                    details: e.to_string(),
                })?;

            // Reject symlinks.
            if let Some(mode) = entry.unix_mode() {
                if mode & 0o170000 == 0o120000 {
                    return Err(Error::ModpackOverridesPathEscape { entry: rel });
                }
            }

            if !crate::mods::modpack::path_safety::is_safe_relative_path(&rel) {
                return Err(Error::ModpackOverridesPathEscape { entry: rel });
            }

            if entry.is_dir() {
                EntryKind::Dir
            } else {
                // The declared `entry.size()` is attacker-controlled (central
                // directory field), so it is only a cheap early reject — never
                // the source of truth for the caps. Read the body through a
                // `take(PER_FILE_CAP + 1)` limiter and enforce the caps on the
                // ACTUAL bytes read.
                let declared = entry.size();
                if declared > PER_FILE_CAP {
                    // Declared oversize: skip without reading (so a zip-bomb's
                    // declared size never forces an allocation). An oversized
                    // override is an inert non-mod blob (MC loads `mods/*.jar`
                    // only); the rest of the pack installs and the user is told
                    // what was left out.
                    EntryKind::Oversized(declared)
                } else {
                    // Cap-bounded read: at most PER_FILE_CAP + 1 bytes. If the
                    // limiter yields more than PER_FILE_CAP the declared size
                    // lied — treat it as oversized (skip), matching the
                    // declared-oversize branch.
                    let mut buf = Vec::new();
                    let read = entry
                        .by_ref()
                        .take(PER_FILE_CAP + 1)
                        .read_to_end(&mut buf)
                        .map_err(|e| Error::Io {
                            path: rel.clone(),
                            details: e.to_string(),
                        })? as u64;
                    if read > PER_FILE_CAP {
                        EntryKind::Oversized(read)
                    } else {
                        // Debit the aggregate by the ACTUAL bytes, not the
                        // declared size, then enforce the aggregate cap.
                        aggregate = aggregate.saturating_add(read);
                        if aggregate > AGGREGATE_CAP {
                            return Err(Error::ModpackOverridesTooLarge {
                                entry: "<aggregate>".into(),
                                size: aggregate as f64,
                                cap: AGGREGATE_CAP as f64,
                            });
                        }
                        EntryKind::File(buf)
                    }
                }
            }
            // `entry` (ZipFile<'_>) goes out of scope here — no `.await`
            // beyond this point holds it.
        };

        let target = mc_dir.join(&rel);
        match kind {
            EntryKind::Oversized(size) => {
                // Nothing written. Record it so the import surfaces a
                // non-fatal "skipped" note instead of failing.
                skipped.push(SkippedOverride {
                    path: rel,
                    size: size as f64,
                });
            }
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
                // Temp-then-rename, not `File::create`: re-importing pack files
                // extracts into an EXISTING instance, and an
                // `overrides/mods/*.jar` target may be a hardlink shared with
                // other instances — truncating it in place would corrupt that
                // mod everywhere. `place_bytes` also keeps the explicit flush a
                // tokio `File` needs (it does not flush on drop, which once
                // truncated overrides on Linux/macOS — the root of the
                // `extracts_normal_file` failure, and a real data bug).
                crate::mods::store::place_bytes(&target, &buf)
                    .await
                    .map_err(|e| Error::Io {
                        path: e.path.display().to_string(),
                        details: e.details(),
                    })?;

                if is_tracked_bundled_path(&rel) {
                    let filename = rel.rsplit('/').next().unwrap_or(&rel).to_string();
                    let sha1 = hex::encode(Sha1::digest(&buf));
                    extracted.push(ExtractedAsset {
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
    Ok(ExtractOutcome { extracted, skipped })
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
    async fn returns_extracted_assets_for_mods_resourcepacks_shaders() {
        // extract() surfaces bundled jars under mods/, and TOP-LEVEL
        // files under resourcepacks/ and shaderpacks/. Folder-form
        // resourcepacks and bulk config files are extracted to disk but
        // NOT surfaced (not itemised in pack_origin).
        let zip = make_zip(&[
            ("overrides/mods/foo.jar", b"foo-bytes" as &[u8]),
            ("overrides/resourcepacks/RP.zip", b"rp-bytes" as &[u8]),
            ("overrides/shaderpacks/Sh.zip", b"sh-bytes" as &[u8]),
            ("overrides/resourcepacks/Folder/pack.mcmeta", b"{}" as &[u8]),
            ("overrides/config/foo.toml", b"k=v" as &[u8]),
            ("overrides/mods/notes.txt", b"not-a-jar" as &[u8]),
            // Non-loadable blobs an author left in the asset dirs: a .zip.txt
            // download note and a stray .txt. Extracted to disk, but NOT
            // itemised as a resourcepack/shader.
            (
                "overrides/shaderpacks/Sh.zip.txt",
                b"download here" as &[u8],
            ),
            ("overrides/resourcepacks/readme.txt", b"notes" as &[u8]),
        ]);
        let inst = TempDir::new().unwrap();
        let out = extract(&zip, inst.path(), |_, _| {}).await.unwrap();
        let paths: std::collections::HashSet<&str> = out
            .extracted
            .iter()
            .map(|a| a.install_path.as_str())
            .collect();
        assert_eq!(
            paths,
            ["mods/foo.jar", "resourcepacks/RP.zip", "shaderpacks/Sh.zip"]
                .into_iter()
                .collect(),
            "got {:?}",
            out.extracted
        );
    }

    #[tokio::test]
    async fn per_file_cap_skips_oversized_does_not_abort() {
        // A pack with a normal mod jar PLUS one oversized blob (201 MiB,
        // > PER_FILE_CAP) — exactly the "mods.rar an author left in mods/"
        // shape. The oversized file must be skipped (recorded, not written)
        // while the normal file extracts; the import must NOT error.
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            w.start_file("overrides/mods/real.jar", SimpleFileOptions::default())
                .unwrap();
            w.write_all(b"real-mod-bytes").unwrap();
            w.start_file("overrides/mods/mods.rar", SimpleFileOptions::default())
                .unwrap();
            let block = vec![0u8; 1024 * 1024];
            for _ in 0..201 {
                w.write_all(&block).unwrap();
            }
            w.finish().unwrap();
        }
        let inst = TempDir::new().unwrap();
        let out = extract(&buf, inst.path(), |_, _| {}).await.unwrap();

        // Normal jar extracted to disk and surfaced as a bundled asset.
        assert_eq!(
            tokio::fs::read(inst.path().join(".minecraft/mods/real.jar"))
                .await
                .unwrap(),
            b"real-mod-bytes"
        );
        assert!(out
            .extracted
            .iter()
            .any(|a| a.install_path == "mods/real.jar"));

        // Oversized blob skipped: recorded, never written to disk.
        assert_eq!(out.skipped.len(), 1);
        assert_eq!(out.skipped[0].path, "mods/mods.rar");
        assert!(out.skipped[0].size > PER_FILE_CAP as f64);
        assert!(
            !inst.path().join(".minecraft/mods/mods.rar").exists(),
            "oversized override must not be written"
        );
    }
}

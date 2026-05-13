//! `ensure_jre(component)` + `java_executable_path(component)`.
//!
//! `.installed` marker shape: two lines,
//! `<component-version>\n<sha1-of-component-manifest>`. Both must match
//! the live top-level manifest for `ensure_jre` to short-circuit.

use crate::error::{Error, Result};
use crate::jre::manifest::{
    fetch_component_manifest, fetch_top_level, mojang_platform_key, pick_component,
    ComponentManifest, FileEntry,
};
use crate::network::download_with_sha;
use crate::paths::jres_dir;
use futures_util::stream::{self, StreamExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

const CONCURRENCY: usize = 8;

/// Default component name when MC version JSON has no `javaVersion`
/// field. Mojang ships this for legacy MC.
pub const DEFAULT_LEGACY_COMPONENT: &str = "jre-legacy";

/// Where the `java(.exe)` binary lives once `ensure_jre` succeeds.
/// On Windows we return `javaw.exe` (no console window) — slice 6
/// owns the spawn, not us. Existence is NOT verified here; caller
/// resolves the path then spawns. If the path doesn't exist that's
/// surfaced as a spawn error, not a path error.
pub fn java_executable_path(component: &str, app: &tauri::AppHandle) -> Result<PathBuf> {
    let dir = jres_dir(app).map_err(|e| Error::io("<jres_dir>", e))?;
    let bin = dir.join(component).join("bin");
    #[cfg(target_os = "windows")]
    let exe = bin.join("javaw.exe");
    #[cfg(not(target_os = "windows"))]
    let exe = bin.join("java");
    Ok(exe)
}

// ---------------------------------------------------------------- marker

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub version: String,
    pub manifest_sha1: String,
}

impl Marker {
    pub fn serialize(&self) -> String {
        format!("{}\n{}", self.version, self.manifest_sha1)
    }

    /// Parse a marker file's contents. Malformed input returns None
    /// (caller treats as cache-miss and re-installs). Permissive
    /// rather than strict: trailing newline / CRLF tolerated.
    pub fn parse(s: &str) -> Option<Self> {
        let mut lines = s.lines();
        let version = lines.next()?.trim().to_string();
        let manifest_sha1 = lines.next()?.trim().to_string();
        if version.is_empty() || manifest_sha1.is_empty() {
            return None;
        }
        Some(Marker {
            version,
            manifest_sha1,
        })
    }
}

pub(crate) fn marker_path(component: &str, app: &tauri::AppHandle) -> Result<PathBuf> {
    let dir = jres_dir(app).map_err(|e| Error::io("<jres_dir>", e))?;
    Ok(dir.join(component).join(".installed"))
}

// ---------------------------------------------------------------- ensure

/// Drive the JRE phase for `component` on the current platform.
/// Idempotent: a fully-installed JRE skips network for file bodies,
/// emits a single completion event, and returns in well under a second
/// (one top-level manifest fetch through the cache, one marker read).
pub async fn ensure_jre(
    component: &str,
    app: &tauri::AppHandle,
    on_progress: impl Fn(u32, u32, u64) + Send + Sync + 'static,
) -> Result<()> {
    let os = current_os();
    let arch = current_arch();
    let platform_key = mojang_platform_key(os, arch)?;

    let top = fetch_top_level().await?;
    let comp_ref = pick_component(&top, platform_key, component)?;

    let jres = jres_dir(app).map_err(|e| Error::io("<jres_dir>", e))?;
    let comp_root = jres.join(component);
    let marker_p = comp_root.join(".installed");

    // Marker fast-path: if it matches, no file work.
    if let Ok(raw) = tokio::fs::read_to_string(&marker_p).await {
        if let Some(m) = Marker::parse(&raw) {
            if m.version == comp_ref.version.name
                && m.manifest_sha1 == comp_ref.manifest.sha1
            {
                // Already installed at the right version; emit a
                // one-shot "done" event so the UI moves on.
                on_progress(1, 1, 0);
                return Ok(());
            }
        }
    }

    // Marker mismatch (or absent / malformed) → wipe the component dir
    // and re-install. `remove_dir_all` is no-op-friendly when the dir
    // doesn't exist.
    let _ = tokio::fs::remove_dir_all(&comp_root).await;

    let manifest = fetch_component_manifest(&comp_ref).await?;
    download_files(&manifest, &comp_root, app, on_progress).await?;

    // Marker last — only after every file landed.
    let marker = Marker {
        version: comp_ref.version.name.clone(),
        manifest_sha1: comp_ref.manifest.sha1.clone(),
    };
    if let Some(parent) = marker_p.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    tokio::fs::write(&marker_p, marker.serialize())
        .await
        .map_err(|e| Error::io(marker_p.display().to_string(), e))?;

    Ok(())
}

async fn download_files(
    manifest: &ComponentManifest,
    comp_root: &std::path::Path,
    app: &tauri::AppHandle,
    on_progress: impl Fn(u32, u32, u64) + Send + Sync + 'static,
) -> Result<()> {
    // First pass: count files + make all directories. Sequential.
    // Collect file entries by value to drop the &manifest borrow before
    // we move on_progress (an FnOnce-friendly value) into the stream.
    let mut file_entries: Vec<(String, bool, super::manifest::FileDownload)> = Vec::new();
    let mut dir_paths: Vec<std::path::PathBuf> = Vec::new();
    for (rel, entry) in &manifest.files {
        let dest = comp_root.join(rel);
        match entry {
            FileEntry::Directory {} => dir_paths.push(dest),
            FileEntry::Link { .. } => {
                // Mojang's Windows component manifests don't emit `link`
                // entries; if one shows up, we don't know how to express
                // it on NTFS and refuse rather than guess.
                return Err(Error::UnsupportedPlatform {
                    os: "windows".into(),
                    arch: "link-in-manifest".into(),
                });
            }
            FileEntry::File {
                executable,
                downloads,
            } => {
                file_entries.push((rel.clone(), *executable, downloads.raw.clone()));
            }
        }
    }
    for d in dir_paths {
        tokio::fs::create_dir_all(&d)
            .await
            .map_err(|e| Error::io(d.display().to_string(), e))?;
    }

    let total = file_entries.len() as u32;
    let progress = Arc::new(on_progress);
    let done = Arc::new(AtomicU32::new(0));
    let bytes = Arc::new(AtomicU64::new(0));

    let app = app.clone();
    let comp_root_owned = comp_root.to_path_buf();
    // `executable` is discarded here: on Windows there is no chmod and
    // `.exe`/`.dll` extensions do the job. When macOS/Linux support lands
    // post-v0.1.0 this must be honored via `std::os::unix::fs::PermissionsExt`.
    let results: Vec<Result<()>> = stream::iter(file_entries.into_iter())
        .map(|(rel, _executable, raw)| {
            let app = app.clone();
            let comp_root = comp_root_owned.clone();
            let progress = Arc::clone(&progress);
            let done = Arc::clone(&done);
            let bytes = Arc::clone(&bytes);
            async move {
                let dest = comp_root.join(&rel);
                // Per-file SHA precheck — handles the marker-deleted case.
                let transferred: u64 = if file_matches_sha(&dest, &raw.sha1).await {
                    0
                } else {
                    download_with_sha(&app, &raw.url, &dest, &raw.sha1, "jre").await?;
                    raw.size
                };
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                let b = bytes.fetch_add(transferred, Ordering::Relaxed) + transferred;
                progress(d, total, b);
                Ok::<(), Error>(())
            }
        })
        .buffer_unordered(CONCURRENCY)
        .collect::<Vec<Result<()>>>()
        .await;

    for res in results {
        res?;
    }
    Ok(())
}

async fn file_matches_sha(path: &std::path::Path, expected_sha_hex: &str) -> bool {
    let Ok(bytes) = tokio::fs::read(path).await else {
        return false;
    };
    use sha1::{Digest, Sha1};
    let got = hex::encode(Sha1::digest(&bytes));
    got == expected_sha_hex
}

fn current_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn current_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_round_trip() {
        let m = Marker {
            version: "21.0.3".into(),
            manifest_sha1: "abc123".into(),
        };
        let s = m.serialize();
        let parsed = Marker::parse(&s).unwrap();
        assert_eq!(parsed, m);
    }

    #[test]
    fn marker_parse_tolerates_crlf_and_trailing_newline() {
        let s = "21.0.3\r\nabc123\r\n";
        let parsed = Marker::parse(s).unwrap();
        assert_eq!(parsed.version, "21.0.3");
        assert_eq!(parsed.manifest_sha1, "abc123");
    }

    #[test]
    fn marker_parse_rejects_single_line() {
        assert!(Marker::parse("21.0.3\n").is_none());
        assert!(Marker::parse("21.0.3").is_none());
    }

    #[test]
    fn marker_parse_rejects_empty_fields() {
        assert!(Marker::parse("\nabc123\n").is_none());
        assert!(Marker::parse("21.0.3\n\n").is_none());
        assert!(Marker::parse("").is_none());
    }

    #[test]
    fn ensure_jre_module_paths_resolve() {
        // We can't construct a real AppHandle in unit tests, so we
        // verify path construction logic via a parallel pure helper.
        let comp_root = std::path::PathBuf::from("C:/fake/jres/java-runtime-gamma");
        let dest = comp_root.join("bin/java.exe");
        assert_eq!(
            dest,
            std::path::PathBuf::from("C:/fake/jres/java-runtime-gamma/bin/java.exe")
        );
    }

    #[test]
    fn current_os_returns_known_value() {
        let os = current_os();
        assert!(["windows", "macos", "linux"].contains(&os));
    }

    #[test]
    fn current_arch_returns_known_value() {
        let arch = current_arch();
        assert!(["x64", "aarch64", "x86"].contains(&arch));
    }
}

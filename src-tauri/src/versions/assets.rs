//! Asset index + asset object downloads.
//!
//! Asset index JSON shape:
//! ```json
//! {
//!   "objects": {
//!     "minecraft/lang/en_us.json": {"hash": "abc123...", "size": 1234},
//!     ...
//!   }
//! }
//! ```
//!
//! Each object is stored at `<assets_dir>/objects/<2hex>/<full-hash>`
//! and SHA-1-verified against `hash`. We download up to 8 at a time.

use crate::error::{Error, Result};
use crate::network::download_with_sha;
use crate::paths::assets_dir;
use crate::versions::version_json::AssetIndexRef;
use futures_util::stream::{self, StreamExt, TryStreamExt};
use serde::Deserialize;

// `pub(crate)` so the install report derives its assets row's host from THIS
// constant instead of repeating the literal — a second copy would silently
// start lying the day this one changes.
pub(crate) const ASSET_BASE_URL: &str = "https://resources.download.minecraft.net";
const CONCURRENCY: usize = 8;

#[derive(Debug, Deserialize)]
pub struct AssetIndex {
    pub objects: std::collections::HashMap<String, AssetObject>,
    /// Legacy 1.6–1.7.2 indexes set `virtual: true`: the game reads assets
    /// from a directory tree keyed by *logical path*, not by hash. We must
    /// materialise a copy at `<assets>/virtual/<index_id>/<logical_path>`.
    #[serde(default, rename = "virtual")]
    pub is_virtual: bool,
    /// 1.7.3–1.7.9 set `map_to_resources: true`: the game reads assets from
    /// `<game_dir>/resources/<logical_path>` instead. Same materialisation,
    /// different destination root.
    #[serde(default)]
    pub map_to_resources: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

/// Fetch the asset index JSON (caches to `<assets>/indexes/<id>.json`)
/// and download every missing object concurrently.
///
/// `on_progress(done, total, bytes_done)` is invoked after each object
/// completes — the install orchestrator (Task 6) uses this to emit
/// phase-level events.
///
/// `game_dir` is the instance's Minecraft directory, used ONLY by legacy
/// `map_to_resources` indexes (1.7.3–1.7.9) which materialise hashed objects
/// into `<game_dir>/resources/<logical_path>`. It is `None` when the caller
/// has no instance context (the install pipeline installs a *version*, not an
/// instance); in that case a `map_to_resources` index logs and skips the
/// per-instance mirror — the shared `virtual` case (1.6–1.7.2) still works
/// because its destination is the version-scoped `<assets>/virtual/<id>/`.
pub async fn ensure_assets(
    asset_index: &AssetIndexRef,
    game_dir: Option<&std::path::Path>,
    app: &tauri::AppHandle,
    on_progress: impl Fn(u32, u32, u64) + Send + Sync + 'static,
) -> Result<()> {
    let root = assets_dir(app).map_err(|e| Error::io("<assets_dir>", e))?;
    let indexes_dir = root.join("indexes");
    let objects_dir = root.join("objects");

    let index_file = indexes_dir.join(format!("{}.json", asset_index.id));
    if !file_matches_sha(&index_file, &asset_index.sha1).await {
        download_with_sha(
            app,
            &asset_index.url,
            &index_file,
            &asset_index.sha1,
            "assets",
        )
        .await?;
    }

    let raw = tokio::fs::read(&index_file)
        .await
        .map_err(|e| Error::io(index_file.display().to_string(), e))?;
    let index: AssetIndex = serde_json::from_slice(&raw)
        .map_err(|e| Error::io(index_file.display().to_string(), format!("parse: {e}")))?;

    // Legacy indexes need the hashed objects mirrored into a by-logical-path
    // tree. Resolve that destination root now (before we consume `objects`).
    let materialise_root: Option<std::path::PathBuf> = if index.map_to_resources {
        match game_dir {
            Some(gd) => Some(gd.join("resources")),
            None => {
                crate::diag!(
                    "assets: index {} is map_to_resources but no game_dir was supplied — \
                     skipping per-instance resources mirror (legacy 1.7.3–1.7.9)",
                    asset_index.id
                );
                None
            }
        }
    } else if index.is_virtual {
        Some(root.join("virtual").join(&asset_index.id))
    } else {
        None
    };

    let total = index.objects.len() as u32;
    let progress = std::sync::Arc::new(on_progress);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let materialise_root = std::sync::Arc::new(materialise_root);

    let app = app.clone();
    stream::iter(index.objects.into_iter())
        .map(|(logical_path, obj)| {
            let app = app.clone();
            let objects_dir = objects_dir.clone();
            let progress = std::sync::Arc::clone(&progress);
            let done = std::sync::Arc::clone(&done);
            let bytes = std::sync::Arc::clone(&bytes);
            let materialise_root = std::sync::Arc::clone(&materialise_root);
            async move {
                use std::sync::atomic::Ordering;
                let prefix = &obj.hash[..2];
                let dest = objects_dir.join(prefix).join(&obj.hash);
                let url = format!("{ASSET_BASE_URL}/{prefix}/{}", obj.hash);
                // Skipped files contribute 0 bytes to `bytes_done` so the UI's
                // "MB downloaded" counter reflects bytes actually transferred,
                // not bytes a fully-installed re-run could have transferred.
                let transferred = if file_matches_sha(&dest, &obj.hash).await {
                    0
                } else {
                    download_with_sha(&app, &url, &dest, &obj.hash, "assets").await?;
                    obj.size
                };
                // Legacy virtual / map_to_resources: mirror the hashed object
                // to its logical path so the game can find it by name.
                if let Some(base) = materialise_root.as_ref() {
                    materialise_object(&dest, base, &logical_path).await?;
                }
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                let b = bytes.fetch_add(transferred, Ordering::Relaxed) + transferred;
                progress(d, total, b);
                Ok::<(), Error>(())
            }
        })
        .buffer_unordered(CONCURRENCY)
        // Short-circuit on the first error: `try_collect` stops consuming and
        // drops the remaining in-flight downloads, rather than running every
        // task to completion before surfacing the earliest failure.
        .try_collect::<Vec<()>>()
        .await?;
    Ok(())
}

/// Copy a hashed asset object to `<base>/<logical_path>` for legacy
/// virtual/map_to_resources indexes. Idempotent: skips the copy when the
/// destination already matches by size (cheap) — a full re-hash per object
/// would defeat the point of the object-store skip above. `logical_path` is
/// index-controlled data, so it is confined to `base` with `enclosed_name`.
async fn materialise_object(
    object_path: &std::path::Path,
    base: &std::path::Path,
    logical_path: &str,
) -> Result<()> {
    // Confine the logical path under `base`: reject `..`/rooted names.
    let Some(rel) = confined_relative(logical_path) else {
        crate::diag!("assets: skipping unsafe logical asset path {logical_path:?}");
        return Ok(());
    };
    let out = base.join(rel);
    // Skip if already present with the same length (assets are content-named,
    // so a size match on the same logical path is a reliable up-to-date signal).
    if let (Ok(src_meta), Ok(dst_meta)) = (
        tokio::fs::metadata(object_path).await,
        tokio::fs::metadata(&out).await,
    ) {
        if src_meta.len() == dst_meta.len() {
            return Ok(());
        }
    }
    if let Some(parent) = out.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    tokio::fs::copy(object_path, &out)
        .await
        .map_err(|e| Error::io(out.display().to_string(), e))?;
    Ok(())
}

/// Return a relative path confined under a root, or `None` if the input
/// contains `..` traversal, a rooted/absolute component, or is empty.
fn confined_relative(logical_path: &str) -> Option<std::path::PathBuf> {
    use std::path::Component;
    let candidate = std::path::Path::new(logical_path);
    let mut out = std::path::PathBuf::new();
    for comp in candidate.components() {
        match comp {
            Component::Normal(part) => out.push(part),
            // Anything else (RootDir, Prefix, ParentDir, CurDir) escapes or is
            // meaningless for a confined relative path.
            _ => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

async fn file_matches_sha(path: &std::path::Path, expected_sha_hex: &str) -> bool {
    let Ok(bytes) = tokio::fs::read(path).await else {
        return false;
    };
    use sha1::{Digest, Sha1};
    let got = hex::encode(Sha1::digest(&bytes));
    got == expected_sha_hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_asset_index_shape() {
        let json = r#"{
          "objects": {
            "minecraft/lang/en_us.json": {"hash": "aaa111", "size": 1234},
            "minecraft/sounds/ambient/cave.ogg": {"hash": "bbb222", "size": 5678}
          }
        }"#;
        let index: AssetIndex = serde_json::from_str(json).unwrap();
        assert_eq!(index.objects.len(), 2);
        let lang = &index.objects["minecraft/lang/en_us.json"];
        assert_eq!(lang.hash, "aaa111");
        assert_eq!(lang.size, 1234);
    }

    #[test]
    fn parses_legacy_virtual_flag() {
        let json = r#"{ "virtual": true, "objects": {} }"#;
        let index: AssetIndex = serde_json::from_str(json).unwrap();
        assert!(index.is_virtual);
        assert!(!index.map_to_resources);
    }

    #[test]
    fn parses_map_to_resources_flag() {
        let json = r#"{ "map_to_resources": true, "objects": {} }"#;
        let index: AssetIndex = serde_json::from_str(json).unwrap();
        assert!(index.map_to_resources);
        assert!(!index.is_virtual);
    }

    #[test]
    fn modern_index_defaults_both_flags_false() {
        let json = r#"{ "objects": {} }"#;
        let index: AssetIndex = serde_json::from_str(json).unwrap();
        assert!(!index.is_virtual);
        assert!(!index.map_to_resources);
    }

    #[test]
    fn confined_relative_accepts_nested_logical_path() {
        let p = confined_relative("minecraft/lang/en_us.lang").unwrap();
        assert_eq!(p, std::path::Path::new("minecraft/lang/en_us.lang"));
    }

    #[test]
    fn confined_relative_rejects_traversal_and_rooted() {
        assert!(confined_relative("../escape").is_none());
        assert!(confined_relative("a/../../b").is_none());
        assert!(confined_relative("/abs/path").is_none());
        assert!(confined_relative("").is_none());
    }

    #[test]
    fn asset_url_uses_two_char_prefix() {
        // Verify the URL construction convention: "abc123..." → "ab/abc123..."
        let hash = "abc123def456";
        let prefix = &hash[..2];
        let url = format!("{ASSET_BASE_URL}/{prefix}/{hash}");
        assert_eq!(
            url,
            "https://resources.download.minecraft.net/ab/abc123def456"
        );
    }
}

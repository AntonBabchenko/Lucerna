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

const ASSET_BASE_URL: &str = "https://resources.download.minecraft.net";
const CONCURRENCY: usize = 8;

#[derive(Debug, Deserialize)]
pub struct AssetIndex {
    pub objects: std::collections::HashMap<String, AssetObject>,
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
pub async fn ensure_assets(
    asset_index: &AssetIndexRef,
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

    let total = index.objects.len() as u32;
    let progress = std::sync::Arc::new(on_progress);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

    let app = app.clone();
    stream::iter(index.objects.into_iter())
        .map(|(_logical_path, obj)| {
            let app = app.clone();
            let objects_dir = objects_dir.clone();
            let progress = std::sync::Arc::clone(&progress);
            let done = std::sync::Arc::clone(&done);
            let bytes = std::sync::Arc::clone(&bytes);
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

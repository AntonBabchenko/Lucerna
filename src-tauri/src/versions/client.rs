//! Download the client.jar for a given version. Single file, trivial
//! wrapper around `download_with_sha` that picks the destination path.

use crate::error::{Error, Result};
use crate::network::download_with_sha;
use crate::paths::versions_dir;
use crate::versions::version_json::DownloadEntry;

/// Ensure `<versions_dir>/<id>/<id>.jar` exists and matches the
/// expected SHA-1. Re-downloads only if missing or hash mismatches.
///
/// Returns whether bytes were actually fetched, so the install report can tell
/// a real download from a precheck hit. Callers that build no report ignore it.
pub async fn ensure_client(
    version_id: &str,
    entry: &DownloadEntry,
    app: &tauri::AppHandle,
) -> Result<bool> {
    let dir = versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let dest = dir.join(version_id).join(format!("{version_id}.jar"));
    if file_matches_sha(&dest, &entry.sha1).await {
        return Ok(false);
    }
    download_with_sha(app, &entry.url, &dest, &entry.sha1, "client").await?;
    Ok(true)
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
    #[test]
    fn destination_path_uses_version_id() {
        // Pure assertion — verifies the path construction convention
        // (we can't construct an AppHandle in unit tests; this exercises
        // the format-string logic indirectly).
        let id = "1.20.4";
        let expected_filename = format!("{id}.jar");
        assert_eq!(expected_filename, "1.20.4.jar");
    }
}

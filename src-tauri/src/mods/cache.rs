//! Global mod jar cache. Content-addressed by SHA-1.
//!
//! Layout: `{data_dir}/mod-cache/<sha1>.jar`. The same SHA may be
//! referenced by multiple per-instance installs; each install copies
//! out of the cache (cheap) rather than linking, so per-instance
//! deletion never affects the cache.

use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::Error;

pub fn cache_root(data_dir: &Path) -> PathBuf {
    data_dir.join("mod-cache")
}

pub fn cache_path_for(data_dir: &Path, sha1_lower: &str) -> PathBuf {
    cache_root(data_dir).join(format!("{sha1_lower}.jar"))
}

/// True if a cache entry exists and its SHA-1 matches `expected`. If
/// the entry exists but the hash mismatches (rare; concurrent write or
/// disk corruption), delete it and return false so the caller falls
/// back to the cold path.
pub async fn verify_or_evict(data_dir: &Path, expected_sha1: &str) -> Result<bool, Error> {
    let path = cache_path_for(data_dir, expected_sha1);
    if !fs::try_exists(&path).await.map_err(io_to_cache_err)? {
        return Ok(false);
    }
    let bytes = fs::read(&path).await.map_err(io_to_cache_err)?;
    let got = hex::encode(Sha1::digest(&bytes));
    if got.eq_ignore_ascii_case(expected_sha1) {
        Ok(true)
    } else {
        fs::remove_file(&path).await.map_err(io_to_cache_err)?;
        Ok(false)
    }
}

/// Atomically write `bytes` to the cache, keyed by SHA-1. Returns the
/// final path. Returns `ModsSha1Mismatch` if the computed hash of
/// `bytes` does not match `expected_sha1`.
pub async fn write_bytes(
    data_dir: &Path,
    expected_sha1: &str,
    bytes: &[u8],
) -> Result<PathBuf, Error> {
    let got = hex::encode(Sha1::digest(bytes));
    if !got.eq_ignore_ascii_case(expected_sha1) {
        return Err(Error::ModsSha1Mismatch {
            expected: expected_sha1.into(),
            got,
        });
    }
    fs::create_dir_all(cache_root(data_dir))
        .await
        .map_err(io_to_cache_err)?;
    let final_path = cache_path_for(data_dir, &got);
    let tmp = final_path.with_extension("jar.tmp");
    let mut f = fs::File::create(&tmp).await.map_err(io_to_cache_err)?;
    f.write_all(bytes).await.map_err(io_to_cache_err)?;
    f.flush().await.map_err(io_to_cache_err)?;
    drop(f);
    fs::rename(&tmp, &final_path)
        .await
        .map_err(io_to_cache_err)?;
    Ok(final_path)
}

pub async fn size_bytes(data_dir: &Path) -> Result<u64, Error> {
    let root = cache_root(data_dir);
    if !fs::try_exists(&root).await.map_err(io_to_cache_err)? {
        return Ok(0);
    }
    let mut total = 0u64;
    let mut rd = fs::read_dir(&root).await.map_err(io_to_cache_err)?;
    while let Some(entry) = rd.next_entry().await.map_err(io_to_cache_err)? {
        let m = entry.metadata().await.map_err(io_to_cache_err)?;
        if m.is_file() {
            total = total.saturating_add(m.len());
        }
    }
    Ok(total)
}

pub async fn clear(data_dir: &Path) -> Result<u64, Error> {
    let root = cache_root(data_dir);
    if !fs::try_exists(&root).await.map_err(io_to_cache_err)? {
        return Ok(0);
    }
    let before = size_bytes(data_dir).await?;
    let mut rd = fs::read_dir(&root).await.map_err(io_to_cache_err)?;
    while let Some(entry) = rd.next_entry().await.map_err(io_to_cache_err)? {
        if entry.metadata().await.map_err(io_to_cache_err)?.is_file() {
            fs::remove_file(entry.path())
                .await
                .map_err(io_to_cache_err)?;
        }
    }
    Ok(before)
}

fn io_to_cache_err(e: std::io::Error) -> Error {
    Error::ModsCacheIo {
        details: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn hash_of(s: &[u8]) -> String {
        hex::encode(Sha1::digest(s))
    }

    #[tokio::test]
    async fn write_and_verify_round_trip() {
        let td = TempDir::new().unwrap();
        let payload = b"hello world";
        let sha = hash_of(payload);
        let path = write_bytes(td.path(), &sha, payload).await.unwrap();
        assert!(path.exists());
        assert_eq!(path, cache_path_for(td.path(), &sha));
        assert!(verify_or_evict(td.path(), &sha).await.unwrap());
    }

    #[tokio::test]
    async fn write_with_wrong_hash_rejects() {
        let td = TempDir::new().unwrap();
        let err = write_bytes(td.path(), "0000000000000000000000000000000000000000", b"hi")
            .await
            .unwrap_err();
        matches!(err, Error::ModsSha1Mismatch { .. });
    }

    #[tokio::test]
    async fn verify_evicts_corrupt_entry() {
        let td = TempDir::new().unwrap();
        let sha = hash_of(b"original");
        write_bytes(td.path(), &sha, b"original").await.unwrap();
        fs::write(cache_path_for(td.path(), &sha), b"corrupted")
            .await
            .unwrap();
        assert!(!verify_or_evict(td.path(), &sha).await.unwrap());
        assert!(!cache_path_for(td.path(), &sha).exists());
    }

    #[tokio::test]
    async fn size_and_clear() {
        let td = TempDir::new().unwrap();
        let s1 = hash_of(b"a");
        let s2 = hash_of(b"bb");
        write_bytes(td.path(), &s1, b"a").await.unwrap();
        write_bytes(td.path(), &s2, b"bb").await.unwrap();
        assert_eq!(size_bytes(td.path()).await.unwrap(), 3);
        let freed = clear(td.path()).await.unwrap();
        assert_eq!(freed, 3);
        assert_eq!(size_bytes(td.path()).await.unwrap(), 0);
    }
}

//! SHA-1 (and md5 when asked) of a file the user dropped or downloaded, read in
//! 64 KiB chunks — never the whole file in memory (spec §4.2 step 3). An
//! in-process memo keyed by `(path, mtime, len)` keeps the folder watch from
//! re-reading an unchanged file every poll.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use md5::Digest as _;
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digests {
    pub sha1: String,
    pub md5: Option<String>,
}

type Memo = HashMap<PathBuf, (SystemTime, u64, Digests)>;
static MEMO: Mutex<Option<Memo>> = Mutex::new(None);

pub async fn digests(path: &Path, want_md5: bool) -> std::io::Result<Digests> {
    let meta = tokio::fs::metadata(path).await?;
    let mtime = meta.modified()?;
    let len = meta.len();
    {
        let memo = MEMO.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((m, l, d)) = memo.as_ref().and_then(|m| m.get(path)) {
            if *m == mtime && *l == len && (d.md5.is_some() || !want_md5) {
                return Ok(d.clone());
            }
        }
    }
    let d = read_digests(path, want_md5).await?;
    MEMO.lock()
        .unwrap_or_else(|p| p.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(path.to_path_buf(), (mtime, len, d.clone()));
    Ok(d)
}

async fn read_digests(path: &Path, want_md5: bool) -> std::io::Result<Digests> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut sha1 = sha1::Sha1::new();
    let mut md5 = want_md5.then(md5::Md5::new);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        sha1::Digest::update(&mut sha1, &buf[..n]);
        if let Some(m) = md5.as_mut() {
            m.update(&buf[..n]);
        }
    }
    Ok(Digests {
        sha1: hex::encode(sha1::Digest::finalize(sha1)),
        md5: md5.map(|m| hex::encode(m.finalize())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn one_pass_gives_sha1_and_md5_and_a_rewrite_is_read_again() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.jar");
        std::fs::write(&p, b"abc").unwrap();
        let d = digests(&p, true).await.unwrap();
        assert_eq!(d.sha1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(d.md5.as_deref(), Some("900150983cd24fb0d6963f7d28e17f72"));
        let no_md5 = digests(&p, false).await.unwrap();
        assert!(no_md5.md5.is_some(), "a cached md5 is still handed back");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&p, b"abcd").unwrap();
        let again = digests(&p, false).await.unwrap();
        assert_ne!(again.sha1, d.sha1, "new bytes, new digest");
    }
}

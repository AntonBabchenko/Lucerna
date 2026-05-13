//! Gzip-aware capped read with tail-on-overflow.
//!
//! `.gz` files stream through `flate2::read::GzDecoder` with a
//! decompressed-byte cap. Plain files larger than the cap return
//! their last <cap> bytes (the tail), where errors usually live.

use crate::error::{Error, Result};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const DEFAULT_CAP_BYTES: u64 = 5 * 1024 * 1024; // 5 MB
pub const MIN_CAP_BYTES: u64 = 64 * 1024; // 64 KB
pub const MAX_CAP_BYTES: u64 = 100 * 1024 * 1024; // 100 MB

/// Read up to `max_bytes` of `path`, decompressing if filename ends
/// `.gz`. For plain files larger than `max_bytes`, returns the tail.
/// `max_bytes` is clamped to `[MIN_CAP_BYTES, MAX_CAP_BYTES]`; 0
/// becomes `DEFAULT_CAP_BYTES`.
pub fn read_with_cap(path: &Path, max_bytes: u64) -> Result<String> {
    let cap = clamp_cap(max_bytes);
    let bytes = if is_gzipped(path) {
        read_gz(path, cap)?
    } else {
        read_plain(path, cap)?
    };
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn clamp_cap(max_bytes: u64) -> u64 {
    if max_bytes == 0 {
        return DEFAULT_CAP_BYTES;
    }
    max_bytes.clamp(MIN_CAP_BYTES, MAX_CAP_BYTES)
}

fn is_gzipped(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("gz"))
        .unwrap_or(false)
}

fn read_plain(path: &Path, cap: u64) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| Error::io(path.display().to_string(), e))?;
    let len = file
        .metadata()
        .map_err(|e| Error::io(path.display().to_string(), e))?
        .len();
    if len > cap {
        // Tail: seek to (len - cap) and read cap bytes.
        file.seek(SeekFrom::Start(len - cap))
            .map_err(|e| Error::io(path.display().to_string(), e))?;
    }
    // Bound the initial allocation: a 5 KB file with a 100 MB cap
    // shouldn't reserve 100 MB up front. Cap the reservation at
    // min(cap, len, 1 MB) — Vec grows past this if needed.
    let initial = cap.min(len).min(1 << 20);
    let mut buf = Vec::with_capacity(initial as usize);
    file.take(cap)
        .read_to_end(&mut buf)
        .map_err(|e| Error::io(path.display().to_string(), e))?;
    Ok(buf)
}

fn read_gz(path: &Path, cap: u64) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)
        .map_err(|e| Error::io(path.display().to_string(), e))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut buf = Vec::with_capacity(cap.min(1 << 20) as usize);
    decoder
        .take(cap)
        .read_to_end(&mut buf)
        .map_err(|e| Error::io(path.display().to_string(), format!("gzip: {e}")))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    #[test]
    fn clamp_zero_becomes_default() {
        assert_eq!(clamp_cap(0), DEFAULT_CAP_BYTES);
    }

    #[test]
    fn clamp_too_small_becomes_min() {
        assert_eq!(clamp_cap(100), MIN_CAP_BYTES);
    }

    #[test]
    fn clamp_too_large_becomes_max() {
        assert_eq!(clamp_cap(u64::MAX), MAX_CAP_BYTES);
    }

    #[test]
    fn clamp_in_range_is_identity() {
        assert_eq!(clamp_cap(1_000_000), 1_000_000);
    }

    #[test]
    fn plain_under_cap_returns_full() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.log");
        std::fs::write(&f, b"hello world").unwrap();
        let s = read_with_cap(&f, MIN_CAP_BYTES).expect("read");
        assert_eq!(s, "hello world");
    }

    #[test]
    fn plain_over_cap_returns_tail() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("big.log");
        // Write 200 KB; cap at 64 KB → tail returned.
        let body = "X".repeat(200 * 1024);
        std::fs::write(&f, body.as_bytes()).unwrap();
        let s = read_with_cap(&f, MIN_CAP_BYTES).expect("read");
        assert_eq!(s.len(), MIN_CAP_BYTES as usize);
        assert!(s.chars().all(|c| c == 'X'));
    }

    #[test]
    fn gz_under_cap_returns_decompressed() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.log.gz");
        {
            let file = std::fs::File::create(&f).unwrap();
            let mut enc = GzEncoder::new(file, Compression::default());
            enc.write_all(b"compressed content").unwrap();
            enc.finish().unwrap();
        }
        let s = read_with_cap(&f, DEFAULT_CAP_BYTES).expect("read");
        assert_eq!(s, "compressed content");
    }

    #[test]
    fn gz_over_cap_returns_truncated_decompressed() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("big.log.gz");
        {
            let file = std::fs::File::create(&f).unwrap();
            let mut enc = GzEncoder::new(file, Compression::default());
            // 200 KB decompressed.
            let body = "Y".repeat(200 * 1024);
            enc.write_all(body.as_bytes()).unwrap();
            enc.finish().unwrap();
        }
        let s = read_with_cap(&f, MIN_CAP_BYTES).expect("read");
        // .take() inside read_gz returns AT MOST cap bytes; decompressed
        // boundary may land mid-buffer. We assert <= cap.
        assert!(s.len() <= MIN_CAP_BYTES as usize);
        assert!(s.chars().all(|c| c == 'Y'));
    }

    #[test]
    fn invalid_utf8_replaced_with_fffd() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("bad.log");
        // 0x80 is an invalid lone UTF-8 continuation byte.
        std::fs::write(&f, &[b'A', 0x80, b'B']).unwrap();
        let s = read_with_cap(&f, MIN_CAP_BYTES).expect("read");
        assert!(s.contains('\u{FFFD}'));
        assert!(s.contains('A') && s.contains('B'));
    }
}

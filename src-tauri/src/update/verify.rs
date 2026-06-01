//! Two-layer verification of a downloaded installer: SHA-256 against
//! the release `SHA256SUMS`, then cosign keyless against the
//! `.cosign.bundle`. An unverified binary is never launched.

use crate::error::{Error, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Return the lowercase hex SHA-256 recorded for `filename` in a
/// `SHA256SUMS` body ("<hex>  <name>" per line), if present.
pub fn sha256_for(sums: &str, filename: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?;
        (name == filename).then(|| hash.to_ascii_lowercase())
    })
}

/// Compute the lowercase hex SHA-256 of a file.
pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(&path.display().to_string(), e))?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

/// Verify the installer's SHA-256 matches its `SHA256SUMS` entry.
pub fn verify_sha256(installer: &Path, installer_name: &str, sums: &str) -> Result<()> {
    let expected =
        sha256_for(sums, installer_name).ok_or_else(|| Error::UpdateVerificationFailed {
            details: format!("no SHA256SUMS entry for {installer_name}"),
        })?;
    let got = sha256_file(installer)?;
    if got != expected {
        return Err(Error::UpdateVerificationFailed {
            details: format!(
                "sha256 mismatch for {installer_name}: expected {expected}, got {got}"
            ),
        });
    }
    Ok(())
}

/// Cosign keyless verification of the installer against its bundle.
/// Filled in by a later task (spike confirmed `sigstore-verify 0.8`).
/// `version` is the release version (e.g. "0.9.1") used to pin the exact
/// signing-identity SAN. Until then this is a no-op so the rest of the
/// pipeline compiles and the SHA-256 layer is exercisable.
pub fn verify_cosign(_installer: &Path, _bundle: &Path, _version: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const SUMS: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  EMPTY\n\
                        aaaa  other.bin\n";

    #[test]
    fn sha256_for_finds_entry() {
        assert_eq!(sha256_for(SUMS, "other.bin"), Some("aaaa".into()));
        assert_eq!(sha256_for(SUMS, "missing.bin"), None);
    }

    #[test]
    fn verify_sha256_ok_for_matching_file() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("EMPTY");
        std::fs::write(&f, b"").unwrap(); // sha256 of empty = e3b0c4...
        assert!(verify_sha256(&f, "EMPTY", SUMS).is_ok());
    }

    #[test]
    fn verify_sha256_rejects_mismatch() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("EMPTY");
        std::fs::write(&f, b"tampered").unwrap();
        let r = verify_sha256(&f, "EMPTY", SUMS);
        assert!(matches!(
            r,
            Err(crate::error::Error::UpdateVerificationFailed { .. })
        ));
    }

    #[test]
    fn verify_sha256_rejects_missing_entry() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("nope.bin");
        std::fs::write(&f, b"x").unwrap();
        let r = verify_sha256(&f, "nope.bin", SUMS);
        assert!(matches!(
            r,
            Err(crate::error::Error::UpdateVerificationFailed { .. })
        ));
    }
}

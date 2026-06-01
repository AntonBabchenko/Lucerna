//! Two-layer verification of a downloaded installer: SHA-256 against
//! the release `SHA256SUMS`, then cosign keyless against the
//! `.cosign.bundle`. An unverified binary is never launched.

use crate::error::{Error, Result};
use sha2::{Digest, Sha256};
use sigstore_verify::trust_root::{TrustedRoot, SIGSTORE_PRODUCTION_TRUSTED_ROOT};
use sigstore_verify::types::Bundle;
use sigstore_verify::{verify, VerificationPolicy};
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
    let bytes = std::fs::read(path).map_err(|e| Error::io(path.display().to_string(), e))?;
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

const COSIGN_ISSUER: &str = "https://token.actions.githubusercontent.com";

/// Cosign keyless verification of the installer against its bundle.
/// Synchronous and CPU/IO-heavy — async callers wrap it in
/// `tokio::task::spawn_blocking`. The trust root is the compiled-in
/// Sigstore production root (no network). The signing identity is pinned
/// to the exact release-tag SAN (sigstore-verify 0.8 has no regex
/// matcher), so a binary signed by any other workflow/tag is rejected.
pub fn verify_cosign(installer: &Path, bundle_path: &Path, version: &str) -> Result<()> {
    let trusted_root = TrustedRoot::from_json(SIGSTORE_PRODUCTION_TRUSTED_ROOT).map_err(|e| {
        Error::UpdateVerificationFailed {
            details: format!("trust root: {e}"),
        }
    })?;

    let bundle_json = std::fs::read_to_string(bundle_path)
        .map_err(|e| Error::io(bundle_path.display().to_string(), e))?;
    let bundle = Bundle::from_json(&bundle_json).map_err(|e| Error::UpdateVerificationFailed {
        details: format!("parse bundle: {e}"),
    })?;

    let artifact =
        std::fs::read(installer).map_err(|e| Error::io(installer.display().to_string(), e))?;

    let identity = format!(
        "https://github.com/AntonBabchenko/Lucerna/.github/workflows/release.yml@refs/tags/v{version}"
    );
    let policy = VerificationPolicy::default()
        .require_identity(identity)
        .require_issuer(COSIGN_ISSUER);

    let result = verify(&artifact, &bundle, &policy, &trusted_root).map_err(|e| {
        Error::UpdateVerificationFailed {
            details: format!("cosign verify: {e}"),
        }
    })?;
    if result.success {
        Ok(())
    } else {
        Err(Error::UpdateVerificationFailed {
            details: format!("cosign rejected: {result:?}"),
        })
    }
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

    #[test]
    fn verify_cosign_accepts_good_bundle() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cosign");
        let r = verify_cosign(
            &base.join("good-blob"),
            &base.join("good.cosign.bundle"),
            "0.9.0",
        );
        assert!(r.is_ok(), "good bundle should verify: {r:?}");
    }

    #[test]
    fn verify_cosign_rejects_tampered_blob() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cosign");
        let dir = tempdir().unwrap();
        let tampered = dir.path().join("tampered");
        let mut bytes = std::fs::read(base.join("good-blob")).unwrap();
        bytes[0] ^= 0xff;
        std::fs::write(&tampered, &bytes).unwrap();
        let r = verify_cosign(&tampered, &base.join("good.cosign.bundle"), "0.9.0");
        assert!(matches!(
            r,
            Err(crate::error::Error::UpdateVerificationFailed { .. })
        ));
    }

    #[test]
    fn verify_cosign_rejects_wrong_version_identity() {
        // A different version pins a different SAN -> identity mismatch.
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cosign");
        let r = verify_cosign(
            &base.join("good-blob"),
            &base.join("good.cosign.bundle"),
            "0.9.999",
        );
        assert!(matches!(
            r,
            Err(crate::error::Error::UpdateVerificationFailed { .. })
        ));
    }
}

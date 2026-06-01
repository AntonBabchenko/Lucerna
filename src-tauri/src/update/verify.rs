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

/// Compute the lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Verify the installer bytes' SHA-256 matches its `SHA256SUMS` entry.
/// Takes the already-read bytes so the caller reads the (tens-of-MB)
/// installer once and both verification layers see the SAME bytes — no
/// double read, and no TOCTOU window between the two checks.
pub fn verify_sha256(installer_bytes: &[u8], installer_name: &str, sums: &str) -> Result<()> {
    let expected =
        sha256_for(sums, installer_name).ok_or_else(|| Error::UpdateVerificationFailed {
            details: format!("no SHA256SUMS entry for {installer_name}"),
        })?;
    let got = sha256_hex(installer_bytes);
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
pub fn verify_cosign(installer_bytes: &[u8], bundle_path: &Path, version: &str) -> Result<()> {
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

    let identity = format!(
        "https://github.com/AntonBabchenko/Lucerna/.github/workflows/release.yml@refs/tags/v{version}"
    );
    let policy = VerificationPolicy::default()
        .require_identity(identity)
        .require_issuer(COSIGN_ISSUER);

    let result = verify(installer_bytes, &bundle, &policy, &trusted_root).map_err(|e| {
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

    // Fixtures: `good-blob` is the real `SHA256SUMS` asset from the public
    // v0.9.0 GitHub release, and `good.cosign.bundle` is its sibling
    // `SHA256SUMS.cosign.bundle` (cosign keyless, signed by the release.yml
    // workflow OIDC identity for tag v0.9.0). A small real signed blob —
    // re-fetch with `gh release download v0.9.0` if ever regenerated.
    const SUMS: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  EMPTY\n\
                        aaaa  other.bin\n";

    fn fixture_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cosign")
    }

    #[test]
    fn sha256_for_finds_entry() {
        assert_eq!(sha256_for(SUMS, "other.bin"), Some("aaaa".into()));
        assert_eq!(sha256_for(SUMS, "missing.bin"), None);
    }

    #[test]
    fn verify_sha256_ok_for_matching_bytes() {
        // sha256 of empty input = e3b0c4...
        assert!(verify_sha256(b"", "EMPTY", SUMS).is_ok());
    }

    #[test]
    fn verify_sha256_rejects_mismatch() {
        let r = verify_sha256(b"tampered", "EMPTY", SUMS);
        assert!(matches!(
            r,
            Err(crate::error::Error::UpdateVerificationFailed { .. })
        ));
    }

    #[test]
    fn verify_sha256_rejects_missing_entry() {
        let r = verify_sha256(b"x", "nope.bin", SUMS);
        assert!(matches!(
            r,
            Err(crate::error::Error::UpdateVerificationFailed { .. })
        ));
    }

    #[test]
    fn verify_cosign_accepts_good_bundle() {
        let base = fixture_dir();
        let bytes = std::fs::read(base.join("good-blob")).unwrap();
        let r = verify_cosign(&bytes, &base.join("good.cosign.bundle"), "0.9.0");
        assert!(r.is_ok(), "good bundle should verify: {r:?}");
    }

    #[test]
    fn verify_cosign_rejects_tampered_blob() {
        let base = fixture_dir();
        let mut bytes = std::fs::read(base.join("good-blob")).unwrap();
        bytes[0] ^= 0xff;
        let r = verify_cosign(&bytes, &base.join("good.cosign.bundle"), "0.9.0");
        assert!(matches!(
            r,
            Err(crate::error::Error::UpdateVerificationFailed { .. })
        ));
    }

    #[test]
    fn verify_cosign_rejects_wrong_version_identity() {
        // A different version pins a different SAN -> identity mismatch.
        let base = fixture_dir();
        let bytes = std::fs::read(base.join("good-blob")).unwrap();
        let r = verify_cosign(&bytes, &base.join("good.cosign.bundle"), "0.9.999");
        assert!(matches!(
            r,
            Err(crate::error::Error::UpdateVerificationFailed { .. })
        ));
    }
}

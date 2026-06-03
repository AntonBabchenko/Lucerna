//! Read-only integrity scan. Hashes on-disk files in parallel, classifies
//! each planned artefact, and aggregates a `VerifyReport`. No disk writes.

use crate::verify::ArtifactStatus;

/// Pure classification. `on_disk_sha` is `None` when the file is absent or
/// unreadable. Empty `expected_sha` ⇒ presence-only (can't be Corrupt).
pub fn classify(exists: bool, on_disk_sha: Option<&str>, expected_sha: &str) -> ArtifactStatus {
    if !exists || on_disk_sha.is_none() {
        return ArtifactStatus::Missing;
    }
    if expected_sha.is_empty() {
        return ArtifactStatus::Ok; // presence-only
    }
    if on_disk_sha == Some(expected_sha) {
        ArtifactStatus::Ok
    } else {
        ArtifactStatus::Corrupt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::ArtifactStatus;

    #[test]
    fn missing_when_absent() {
        assert_eq!(classify(false, None, "aa"), ArtifactStatus::Missing);
    }

    #[test]
    fn ok_when_hash_matches() {
        assert_eq!(classify(true, Some("aa"), "aa"), ArtifactStatus::Ok);
    }

    #[test]
    fn corrupt_when_hash_differs() {
        assert_eq!(classify(true, Some("bb"), "aa"), ArtifactStatus::Corrupt);
    }

    #[test]
    fn presence_only_ok_when_present_and_no_expected_sha() {
        assert_eq!(classify(true, Some("anything"), ""), ArtifactStatus::Ok);
    }

    #[test]
    fn presence_only_missing_when_absent_and_no_expected_sha() {
        assert_eq!(classify(false, None, ""), ArtifactStatus::Missing);
    }
}

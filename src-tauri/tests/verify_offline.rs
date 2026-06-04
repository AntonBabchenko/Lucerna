//! Verify must classify a seeded instance tree with NO network access.

use std::fs;
use tempfile::tempdir;

use lucerna_lib::verify::scan::{hash_planned, PlannedOnDisk};
use lucerna_lib::verify::ArtifactStatus;

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    hex::encode(Sha1::digest(bytes))
}

#[tokio::test]
async fn classifies_seeded_tree_offline() {
    let root = tempdir().unwrap();
    let good = root.path().join("good.bin");
    fs::write(&good, b"hello").unwrap();
    let good_sha = sha1_hex(b"hello");

    let corrupt = root.path().join("corrupt.bin");
    fs::write(&corrupt, b"tampered").unwrap();

    let items = vec![
        PlannedOnDisk {
            abs_path: good.clone(),
            expected_sha: good_sha.clone(),
        },
        PlannedOnDisk {
            abs_path: corrupt.clone(),
            expected_sha: good_sha.clone(),
        },
        PlannedOnDisk {
            abs_path: root.path().join("absent.bin"),
            expected_sha: good_sha,
        },
    ];

    let statuses = hash_planned(items, 8, |_done, _total, _bytes| {}).await;
    assert_eq!(statuses[0], ArtifactStatus::Ok);
    assert_eq!(statuses[1], ArtifactStatus::Corrupt);
    assert_eq!(statuses[2], ArtifactStatus::Missing);
}

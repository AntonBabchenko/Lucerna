//! Instance-side materialization of content-addressed store entries.
//!
//! # The invariant this module exists to hold
//!
//! A mod jar in `<instance>/.minecraft/mods/` is a **hardlink** to the single
//! physical file in `mod-cache/<sha1>.jar`, shared with every other instance
//! using that mod. A hardlink is not a copy: it is a second *name* for one
//! file. Creating, deleting or renaming a name touches only that name, but
//! **opening a name for writing changes the bytes every name sees** —
//! including `fs::copy`, whose destination is opened with truncate, and which
//! therefore zeroes the file for every instance *before* it writes its first
//! byte. A crash in that window leaves every instance with an empty jar.
//!
//! So: **never write into an instance's content directories in place.**
//! Materialize into a sibling temp name, then `rename` onto the destination —
//! which replaces one directory entry and leaves every other link intact.
//!
//! Every write into instance content under `src/mods/` goes through this
//! module; `tests/structural_no_inplace_mods_write.rs` enforces that.
//! Removals do not need routing here: deleting one name provably cannot
//! affect the store entry or another instance's link, so the uninstall sites
//! keep their plain `remove_file`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Test-seam key: when set to `1`, hardlinking reports failure, so the copy
/// fallback can be exercised without a second volume or a non-NTFS filesystem —
/// necessary because a dev machine usually has neither. Like every seam key it
/// also reads the process environment (see [`crate::test_seam::resolve`]), so
/// setting it in production forces plain copies. That is a safe degradation
/// rather than a supported feature: copies are always correct, only larger.
const FORCE_LINK_FAILURE: &str = "LUCERNA_TEST_FORCE_LINK_FAILURE";

/// Distinguishes temp names of concurrent materializations in one directory —
/// same reason `installed::write` carries a sequence in its temp name.
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// How a store entry ended up in the instance.
///
/// Deserialize (not just Serialize): `tasks::DetailOutcome::Installed` embeds
/// this and round-trips through a persisted per-file install report, so a
/// write-only derive here would fail to compile the moment that type gained
/// its own `Deserialize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// One physical file, shared with the store and any other instance.
    Linked,
    /// An independent physical copy (link unsupported here, or policy).
    Copied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPolicy {
    /// Content re-obtainable from a platform, so a shared physical file is
    /// safe: corruption is a re-download, never data loss.
    LinkIfPossible,
    /// Content that exists nowhere else, or that is deliberately kept private.
    /// Always an independent copy.
    ForceCopy,
}

/// An I/O failure with the path it concerns. Deliberately **not** a variant of
/// [`crate::error::Error`]: each caller maps this to the error variant it
/// already returns today, so no error class visible to the frontend changes.
#[derive(Debug)]
pub struct StoreIoError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

impl StoreIoError {
    fn at(path: &Path, source: std::io::Error) -> Self {
        Self {
            path: path.to_path_buf(),
            source,
        }
    }

    /// The underlying I/O message — what callers splice into their `details`.
    pub fn details(&self) -> String {
        self.source.to_string()
    }
}

/// Materialize an already-SHA-verified store entry at `dest`.
///
/// `store_path` must have been verified by the caller (`cache::verify_or_evict`
/// via `fetch_to_cache`); this module does not re-hash. `dest`'s parent must
/// exist. An existing file or link at `dest` is *replaced*, never written
/// through.
pub async fn materialize(
    store_path: &Path,
    dest: &Path,
    policy: LinkPolicy,
) -> Result<Placement, StoreIoError> {
    let tmp = temp_sibling(dest)?;

    let placement = match policy {
        LinkPolicy::ForceCopy => {
            copy_into_temp(store_path, &tmp, dest).await?;
            Placement::Copied
        }
        LinkPolicy::LinkIfPossible => match hard_link(store_path, &tmp).await {
            Ok(()) => Placement::Linked,
            Err(e) => {
                // Not an error: a non-NTFS root, a network share, a permission
                // denial or CreateHardLink's 1023-link ceiling all land here.
                // Logged so a silently-undeduplicated install stays diagnosable.
                crate::diag!(
                    "store: hardlink {} -> {} failed ({e}); falling back to a copy",
                    store_path.display(),
                    tmp.display()
                );
                copy_into_temp(store_path, &tmp, dest).await?;
                Placement::Copied
            }
        },
    };

    commit(&tmp, dest).await?;
    Ok(placement)
}

/// Atomically place bytes that have no store entry — a hand-dropped jar, or an
/// `overrides/` entry unpacked from a pack zip. Same temp-then-rename shape, so
/// it can never write through a link already present at `dest`.
pub async fn place_bytes(dest: &Path, bytes: &[u8]) -> Result<(), StoreIoError> {
    let tmp = temp_sibling(dest)?;
    if let Err(e) = write_temp(&tmp, bytes).await {
        cleanup(&tmp).await;
        return Err(StoreIoError::at(dest, e));
    }
    commit(&tmp, dest).await
}

/// A temp name in the SAME directory as `dest`, so the commit rename is a cheap
/// same-volume move that can never fail cross-device.
fn temp_sibling(dest: &Path) -> Result<PathBuf, StoreIoError> {
    let name = dest.file_name().ok_or_else(|| {
        StoreIoError::at(
            dest,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "destination has no file name",
            ),
        )
    })?;
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut tmp_name = name.to_os_string();
    tmp_name.push(format!(".lucerna-tmp.{}.{seq}", std::process::id()));
    Ok(dest.with_file_name(tmp_name))
}

async fn hard_link(store_path: &Path, tmp: &Path) -> Result<(), std::io::Error> {
    if crate::test_seam::resolve(FORCE_LINK_FAILURE).as_deref() == Some("1") {
        return Err(std::io::Error::other("forced link failure (test seam)"));
    }
    fs::hard_link(store_path, tmp).await
}

async fn copy_into_temp(store_path: &Path, tmp: &Path, dest: &Path) -> Result<(), StoreIoError> {
    match fs::copy(store_path, tmp).await {
        Ok(_) => Ok(()),
        Err(e) => {
            cleanup(tmp).await;
            Err(StoreIoError::at(dest, e))
        }
    }
}

async fn write_temp(tmp: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    let mut f = fs::File::create(tmp).await?;
    f.write_all(bytes).await?;
    // A tokio `File` does NOT flush on drop — without this the tail of a large
    // payload can be lost, which is a real data bug `overrides.rs` already hit.
    f.flush().await?;
    Ok(())
}

/// Replace `dest`'s directory entry with `tmp`'s. This — not the write above —
/// is what keeps other links to the old file intact.
async fn commit(tmp: &Path, dest: &Path) -> Result<(), StoreIoError> {
    if let Err(e) = fs::rename(tmp, dest).await {
        cleanup(tmp).await;
        return Err(StoreIoError::at(dest, e));
    }
    Ok(())
}

async fn cleanup(tmp: &Path) {
    if let Err(e) = fs::remove_file(tmp).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            crate::diag!("store: could not remove temp {}: {e}", tmp.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Proof of physical sharing without any platform-specific API: mutate the
    /// store entry's bytes IN PLACE and see whether the instance path observes
    /// it. Same file ⇒ observed. Independent copy ⇒ not observed. This is the
    /// one place in the codebase allowed to write through a link — it is
    /// asserting the very property production code must never rely on.
    fn same_physical_file(store: &std::path::Path, dest: &std::path::Path) -> bool {
        std::fs::write(store, b"MUTATED-THROUGH-STORE").expect("mutate store entry");
        std::fs::read(dest).expect("read dest") == b"MUTATED-THROUGH-STORE"
    }

    /// Every test that needs `FORCE_LINK_FAILURE` to be **absent** must hold
    /// this. `test_seam::scope` serializes scope *holders* against each other,
    /// but a test that installs no scope is not serialized against one that
    /// does — and `resolve` reads a process-global table, so the forced-failure
    /// scope from a sibling test is visible to whoever is running concurrently.
    /// This is the flake `test_seam`'s own test module warns about ("reusing one
    /// key across both tests would itself be a flake"); it turned CI red on
    /// windows while ubuntu and macos won the race. `test_env_lock` takes the
    /// same mutex `scope` does, which closes the gap. Do NOT combine it with
    /// `scope()` in one test — the mutex is not reentrant.
    fn seam_absent() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    fn seeded(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[tokio::test]
    async fn materialize_links_when_the_filesystem_supports_it() {
        let _lock = seam_absent();
        let td = TempDir::new().unwrap();
        let store = seeded(&td.path().join("mod-cache"), "aa.jar", b"JARBYTES");
        let dest = td.path().join("inst/mods/sodium.jar");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();

        let placement = materialize(&store, &dest, LinkPolicy::LinkIfPossible)
            .await
            .unwrap();

        assert_eq!(placement, Placement::Linked);
        assert!(
            same_physical_file(&store, &dest),
            "must be one physical file"
        );
    }

    #[tokio::test]
    async fn materialize_falls_back_to_a_copy_when_linking_fails() {
        let _seam = crate::test_seam::scope(&[("LUCERNA_TEST_FORCE_LINK_FAILURE", "1")]);
        let td = TempDir::new().unwrap();
        let store = seeded(&td.path().join("mod-cache"), "aa.jar", b"JARBYTES");
        let dest = td.path().join("inst/mods/sodium.jar");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();

        let placement = materialize(&store, &dest, LinkPolicy::LinkIfPossible)
            .await
            .unwrap();

        assert_eq!(placement, Placement::Copied);
        assert_eq!(
            std::fs::read(&dest).unwrap(),
            b"JARBYTES",
            "content must be correct"
        );
        assert!(
            !same_physical_file(&store, &dest),
            "a copy must be independent"
        );
    }

    #[tokio::test]
    async fn force_copy_never_links_even_where_linking_works() {
        // The name claims linking WOULD work here, so the seam must be absent —
        // otherwise this test would pass for the wrong reason.
        let _lock = seam_absent();
        let td = TempDir::new().unwrap();
        let store = seeded(&td.path().join("mod-cache"), "aa.jar", b"PACKBYTES");
        let dest = td.path().join("inst/.minecraft/resourcepacks/x.zip");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();

        let placement = materialize(&store, &dest, LinkPolicy::ForceCopy)
            .await
            .unwrap();

        assert_eq!(placement, Placement::Copied);
        assert!(!same_physical_file(&store, &dest));
    }

    /// The load-bearing invariant. `dest` already holds a link shared with
    /// another instance; materializing DIFFERENT content onto it must replace
    /// only this directory entry.
    #[tokio::test]
    async fn materialize_over_an_existing_link_does_not_write_through() {
        // Holds either way (a copy also lands via rename), but the assertions
        // are about the LINK case, so pin the link path rather than pass by luck.
        let _lock = seam_absent();
        let td = TempDir::new().unwrap();
        let old = seeded(&td.path().join("mod-cache"), "old.jar", b"OLD-SHARED-BYTES");
        let new = seeded(&td.path().join("mod-cache"), "new.jar", b"NEW-BYTES");
        let other_instance = td.path().join("instB/mods/sodium.jar");
        let dest = td.path().join("instA/mods/sodium.jar");
        std::fs::create_dir_all(other_instance.parent().unwrap()).unwrap();
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::hard_link(&old, &other_instance).unwrap();
        std::fs::hard_link(&old, &dest).unwrap();

        materialize(&new, &dest, LinkPolicy::LinkIfPossible)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"NEW-BYTES");
        assert_eq!(
            std::fs::read(&other_instance).unwrap(),
            b"OLD-SHARED-BYTES",
            "the other instance's bytes must be untouched"
        );
        assert_eq!(std::fs::read(&old).unwrap(), b"OLD-SHARED-BYTES");
    }

    #[tokio::test]
    async fn materialize_leaves_no_temp_residue_when_it_fails() {
        let td = TempDir::new().unwrap();
        let missing = td.path().join("mod-cache/absent.jar");
        let dest = td.path().join("inst/mods/sodium.jar");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();

        let err = materialize(&missing, &dest, LinkPolicy::LinkIfPossible)
            .await
            .unwrap_err();

        assert_eq!(err.path, dest);
        let residue: Vec<_> = std::fs::read_dir(dest.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".lucerna-tmp."))
            .collect();
        assert!(residue.is_empty(), "temp residue left behind: {residue:?}");
        assert!(!dest.exists(), "dest must not be created on failure");
    }

    #[tokio::test]
    async fn place_bytes_replaces_a_link_without_writing_through() {
        let td = TempDir::new().unwrap();
        let store = seeded(&td.path().join("mod-cache"), "old.jar", b"OLD-SHARED-BYTES");
        let other_instance = td.path().join("instB/mods/local.jar");
        let dest = td.path().join("instA/mods/local.jar");
        std::fs::create_dir_all(other_instance.parent().unwrap()).unwrap();
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::hard_link(&store, &other_instance).unwrap();
        std::fs::hard_link(&store, &dest).unwrap();

        place_bytes(&dest, b"HAND-DROPPED-JAR").await.unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"HAND-DROPPED-JAR");
        assert_eq!(std::fs::read(&other_instance).unwrap(), b"OLD-SHARED-BYTES");
    }

    /// Guards the trap `overrides.rs` already documents: a tokio `File` does
    /// NOT flush on drop, so a missing explicit flush truncates the tail.
    #[tokio::test]
    async fn place_bytes_writes_the_whole_payload() {
        let td = TempDir::new().unwrap();
        let dest = td.path().join("inst/mods/big.jar");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        let payload = vec![7u8; 512 * 1024];

        place_bytes(&dest, &payload).await.unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), payload);
    }
}

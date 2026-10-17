//! Put a named version jar (spec 2026-10-16 §3.2) on disk before a launch: a
//! link to, or else a copy of, the vanilla client jar, under the name the
//! profile's ignore list expects. The vanilla jar itself (`VersionJar::Vanilla`)
//! is passed through untouched, as before.

use crate::error::{Error, Result};
use crate::launch::classpath::VersionJar;
use std::path::PathBuf;

/// One process-wide lock: two instances on the same loader version may launch
/// at once, and both may find the jar absent. The second then sees the first's
/// finished file instead of renaming over it.
fn lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// What reading the named jar said about it.
#[derive(Debug, PartialEq, Eq)]
enum DestState {
    /// Present with the expected checksum: use it.
    Current,
    /// Present with another checksum: rebuild it.
    Stale,
    /// Not there: build it.
    Absent,
}

/// Judge the named jar from its hash read. A read that failed for any reason
/// other than "not there" is passed back as an error: a file we could not read
/// is never rebuilt over (spec 2026-10-16 §3.3, discrimination).
fn judge_dest(read: std::io::Result<Option<String>>, expected: &str) -> std::io::Result<DestState> {
    match read {
        Ok(Some(got)) if got == expected => Ok(DestState::Current),
        Ok(Some(_)) => Ok(DestState::Stale),
        Ok(None) => Ok(DestState::Absent),
        Err(e) => Err(e),
    }
}

/// The jar to put last on `-cp`. A `Vanilla` jar is returned as is, as before
/// this change: install and verify own it. A `Named` jar is checked against the
/// vanilla client checksum on every launch and rebuilt from the vanilla jar when
/// absent or stale. It goes stale when a repair replaces the vanilla jar it was
/// linked to.
///
/// Refuses, never launches without it: no checksum to check against, a source
/// that is missing or damaged (`ClientJarInvalid`), or any file that cannot be
/// read (`Error::io`). A file we could not read is never "rebuilt over".
pub async fn ensure(plan: &VersionJar, mc_version: &str) -> Result<PathBuf> {
    let (dest, source, sha1) = match plan {
        VersionJar::Vanilla(path) => return Ok(path.clone()),
        VersionJar::Named { dest, source, sha1 } => (dest, source, sha1),
    };
    let _guard = lock().lock().await;
    let Some(expected) = sha1.as_deref() else {
        return Err(Error::io(
            dest.display().to_string(),
            "the version names no client checksum, so its version jar cannot be checked",
        ));
    };
    match judge_dest(crate::mods::txn::file_sha1(dest).await, expected) {
        Ok(DestState::Current) => return Ok(dest.clone()),
        Ok(DestState::Stale) => crate::diag!(
            "launch: version jar {} is stale, rebuilding",
            dest.display()
        ),
        Ok(DestState::Absent) => {}
        Err(e) => return Err(Error::io(dest.display().to_string(), e)),
    }
    match crate::mods::txn::file_sha1(source).await {
        Ok(Some(got)) if got == expected => {}
        Ok(Some(_)) => {
            return Err(Error::ClientJarInvalid {
                mc_version: mc_version.to_string(),
                missing: false,
            })
        }
        Ok(None) => {
            return Err(Error::ClientJarInvalid {
                mc_version: mc_version.to_string(),
                missing: true,
            })
        }
        Err(e) => return Err(Error::io(source.display().to_string(), e)),
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let placement = crate::mods::store::materialize(
        source,
        dest,
        crate::mods::store::LinkPolicy::LinkIfPossible,
    )
    .await
    .map_err(|e| Error::io(e.path.display().to_string(), e.details()))?;
    crate::diag!(
        "launch: version jar {} placed ({placement:?})",
        dest.display()
    );
    Ok(dest.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha1::{Digest, Sha1};

    fn sha(bytes: &[u8]) -> String {
        hex::encode(Sha1::digest(bytes))
    }

    fn named(dir: &std::path::Path, sha1: Option<String>) -> VersionJar {
        VersionJar::Named {
            dest: dir.join("forge-x-1.20.1").join("1.20.1-forge-x.jar"),
            source: dir.join("1.20.1").join("1.20.1.jar"),
            sha1,
        }
    }

    fn write_source(dir: &std::path::Path, bytes: &[u8]) {
        std::fs::create_dir_all(dir.join("1.20.1")).unwrap();
        std::fs::write(dir.join("1.20.1").join("1.20.1.jar"), bytes).unwrap();
    }

    #[tokio::test]
    async fn vanilla_is_passed_through_without_touching_disk() {
        let p = PathBuf::from("Z:/nowhere/1.20.1.jar");
        assert_eq!(
            ensure(&VersionJar::Vanilla(p.clone()), "1.20.1")
                .await
                .unwrap(),
            p
        );
    }

    #[tokio::test]
    async fn builds_the_named_jar_from_a_good_source() {
        let td = tempfile::tempdir().unwrap();
        write_source(td.path(), b"vanilla");
        let plan = named(td.path(), Some(sha(b"vanilla")));
        let got = ensure(&plan, "1.20.1").await.unwrap();
        assert_eq!(got, plan.path());
        assert_eq!(std::fs::read(&got).unwrap(), b"vanilla");
    }

    #[tokio::test]
    async fn rebuilds_a_stale_named_jar() {
        let td = tempfile::tempdir().unwrap();
        write_source(td.path(), b"vanilla");
        let plan = named(td.path(), Some(sha(b"vanilla")));
        std::fs::create_dir_all(plan.path().parent().unwrap()).unwrap();
        std::fs::write(plan.path(), b"stale bytes").unwrap();
        ensure(&plan, "1.20.1").await.unwrap();
        assert_eq!(std::fs::read(plan.path()).unwrap(), b"vanilla");
    }

    #[tokio::test]
    async fn a_missing_source_is_client_jar_invalid_missing() {
        let td = tempfile::tempdir().unwrap();
        let plan = named(td.path(), Some(sha(b"vanilla")));
        let err = ensure(&plan, "1.20.1").await.unwrap_err();
        assert!(
            matches!(err, Error::ClientJarInvalid { ref mc_version, missing: true } if mc_version == "1.20.1"),
            "{err:?}"
        );
        assert!(!plan.path().exists(), "nothing placed");
    }

    #[tokio::test]
    async fn a_damaged_source_is_client_jar_invalid_damaged() {
        let td = tempfile::tempdir().unwrap();
        write_source(td.path(), b"corrupted");
        let plan = named(td.path(), Some(sha(b"vanilla")));
        let err = ensure(&plan, "1.20.1").await.unwrap_err();
        assert!(
            matches!(err, Error::ClientJarInvalid { missing: false, .. }),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn no_checksum_is_a_refusal() {
        let td = tempfile::tempdir().unwrap();
        write_source(td.path(), b"vanilla");
        let err = ensure(&named(td.path(), None), "1.20.1").await.unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn a_directory_where_the_jar_should_be_is_an_error() {
        let td = tempfile::tempdir().unwrap();
        write_source(td.path(), b"vanilla");
        let plan = named(td.path(), Some(sha(b"vanilla")));
        // A directory where the jar should be: read fails with something other than NotFound.
        std::fs::create_dir_all(plan.path()).unwrap();
        let err = ensure(&plan, "1.20.1").await.unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn copies_when_a_hardlink_cannot_be_made() {
        let _seam = crate::test_seam::scope(&[("LUCERNA_TEST_FORCE_LINK_FAILURE", "1")]);
        let td = tempfile::tempdir().unwrap();
        write_source(td.path(), b"vanilla");
        let plan = named(td.path(), Some(sha(b"vanilla")));
        let got = ensure(&plan, "1.20.1").await.unwrap();
        assert_eq!(std::fs::read(&got).unwrap(), b"vanilla");
        assert!(
            !crate::file_identity::same_entry(&got, &td.path().join("1.20.1/1.20.1.jar")).unwrap(),
            "a copy, not a link"
        );
    }

    #[test]
    fn a_dest_that_cannot_be_read_is_an_error_not_a_rebuild() {
        let denied = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "locked");
        assert!(judge_dest(Err(denied), "aa").is_err());
    }

    #[test]
    fn a_dest_is_current_stale_or_absent_by_its_hash() {
        assert_eq!(
            judge_dest(Ok(Some("aa".into())), "aa").unwrap(),
            DestState::Current
        );
        assert_eq!(
            judge_dest(Ok(Some("bb".into())), "aa").unwrap(),
            DestState::Stale
        );
        assert_eq!(judge_dest(Ok(None), "aa").unwrap(), DestState::Absent);
    }
}

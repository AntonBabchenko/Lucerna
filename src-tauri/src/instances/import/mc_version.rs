//! The Minecraft version of a profile imported from another launcher
//! (`launcher_import_run`, and the dialog's check before it queues the import):
//! given, and one Lucerna can install — Mojang lists it (`versions::check`).
//!
//! When Mojang's list cannot be loaded, a version already installed on this
//! machine is accepted: a launch reads an installed version's JSON from disk
//! without the list (the disk fast path of `versions::install`), so that profile
//! starts offline. Anything else is refused as unchecked rather than saved
//! unchecked. A server keeps the strict rule (`servers_runtime::mc_version`):
//! its start needs the list anyway.

use std::path::Path;

use crate::error::{Error, Result};
use crate::versions::check::{given, listed, manifest_unreachable};
use crate::versions::VersionEntry;

/// The boundary check. Returns the trimmed id to store.
pub async fn check(versions_dir: &Path, input: &str) -> Result<String> {
    let mc = given(input)?;
    let list = crate::versions::list_manifest().await;
    decide(&mc, list, || installed(versions_dir, &mc))?;
    Ok(mc)
}

/// The decision apart from the fetch and the disk: `list` is what loading
/// Mojang's list gave; `installed` is asked only when it could not be loaded.
fn decide(
    mc: &str,
    list: Result<Vec<VersionEntry>>,
    installed: impl FnOnce() -> bool,
) -> Result<()> {
    match list {
        Ok(known) => listed(mc, &known),
        Err(e) => match manifest_unreachable(e) {
            Error::McVersionUnchecked if installed() => {
                crate::diag!(
                    "instances: accepted Minecraft {mc} for an import without Mojang's version list: it is installed here"
                );
                Ok(())
            }
            other => Err(other),
        },
    }
}

/// Whether this version's JSON is on disk, where the launch reads it. A stat
/// that fails reads as "not installed" — the restrictive answer, since the
/// caller only asks when it could not check. A Mojang id never holds a path
/// separator; one that does is not looked up.
fn installed(versions_dir: &Path, mc: &str) -> bool {
    if mc.contains(['/', '\\']) || mc == "." || mc == ".." {
        return false;
    }
    crate::versions::install::version_json_in(versions_dir, mc).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions::VersionType;

    fn known() -> Vec<VersionEntry> {
        vec![VersionEntry {
            id: "1.20.1".to_string(),
            version_type: VersionType::Release,
            release_date: String::new(),
            url: "https://example.invalid/1.20.1.json".to_string(),
        }]
    }

    fn offline() -> Result<Vec<VersionEntry>> {
        Err(Error::network(
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
            "timed out",
        ))
    }

    #[test]
    fn a_listed_version_passes_without_asking_the_disk() {
        let asked = || -> bool { panic!("the disk is asked only when the list cannot be loaded") };
        assert!(decide("1.20.1", Ok(known()), asked).is_ok());
    }

    #[test]
    fn an_unlisted_version_is_refused_by_name_even_if_installed() {
        match decide("1.20.l", Ok(known()), || true) {
            Err(Error::McVersionUnlisted { mc_version }) => assert_eq!(mc_version, "1.20.l"),
            other => panic!("expected McVersionUnlisted, got {other:?}"),
        }
    }

    #[test]
    fn without_the_list_an_installed_version_passes() {
        assert!(decide("1.20.1", offline(), || true).is_ok());
    }

    #[test]
    fn without_the_list_a_version_not_installed_is_unchecked() {
        assert!(matches!(
            decide("1.20.1", offline(), || false),
            Err(Error::McVersionUnchecked)
        ));
    }

    #[test]
    fn a_list_failure_other_than_the_network_names_itself() {
        let refused = Err(Error::HostNotAllowed {
            url: "https://example.invalid".into(),
        });
        assert!(matches!(
            decide("1.20.1", refused, || true),
            Err(Error::HostNotAllowed { .. })
        ));
    }

    #[test]
    fn installed_means_the_json_the_launch_reads() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!installed(dir.path(), "1.20.1"));
        // The version's folder alone is not an installed version.
        std::fs::create_dir_all(dir.path().join("1.20.1")).unwrap();
        assert!(!installed(dir.path(), "1.20.1"));
        std::fs::write(dir.path().join("1.20.1").join("1.20.1.json"), "{}").unwrap();
        assert!(installed(dir.path(), "1.20.1"));
    }

    #[test]
    fn an_id_with_a_path_in_it_is_never_looked_up() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("inner");
        // Asked for "../x" under `inner`, the layout would read
        // `inner/../x/../x.json`, which is `<dir>/x.json` — planted, so only
        // the guard keeps it from counting.
        std::fs::create_dir_all(dir.path().join("x")).unwrap();
        std::fs::write(dir.path().join("x.json"), "{}").unwrap();
        std::fs::create_dir_all(&inner).unwrap();
        for id in ["../x", "..\\x", "a/b", ".", ".."] {
            assert!(!installed(&inner, id), "{id} must not be looked up");
        }
    }

    #[tokio::test]
    async fn a_blank_version_is_refused_before_any_lookup() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            check(dir.path(), "  ").await,
            Err(Error::McVersionRequired)
        ));
    }
}

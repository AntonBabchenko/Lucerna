//! The Minecraft version of a profile imported from another launcher
//! (`launcher_import_run`, and the dialog's check before it queues the import):
//! given, and one Lucerna can install — Mojang lists it (`versions::check`).
//!
//! When Mojang's list cannot be loaded, a version already installed on this
//! machine — its JSON and its client jar, what `ready_status` asks of it — is
//! accepted: it came from Mojang's list, so it is a real version, and an
//! offline user who has it is not locked out of importing. Anything else is
//! refused as unchecked rather than saved unchecked. A server keeps the strict
//! rule (`servers_runtime::mc_version`): its start needs the list anyway.

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
/// Mojang's list gave; `is_installed` is asked only when it could not be loaded.
fn decide(
    mc: &str,
    list: Result<Vec<VersionEntry>>,
    is_installed: impl FnOnce() -> bool,
) -> Result<()> {
    match list {
        Ok(known) => listed(mc, &known),
        Err(e) => match manifest_unreachable(e) {
            Error::McVersionUnchecked if is_installed() => {
                crate::diag!(
                    "instances: accepted Minecraft {mc} for an import without Mojang's version list: it is installed here"
                );
                Ok(())
            }
            other => Err(other),
        },
    }
}

/// Whether this exact version is installed here: a folder of `versions/` named
/// `mc` exactly, holding the version's JSON and client jar
/// (`versions::install::vanilla_installed_in`). Asked only when Mojang's list
/// could not be loaded, so anything it cannot tell reads as "not installed" —
/// the restrictive answer.
/// - A Mojang id is one plain folder name: anything that could name another
///   path — a separator, a drive prefix (`C:x` replaces the base it is joined
///   to on Windows), `..`, a reserved device name — is never looked up.
/// - The folder is matched by its own name, so a case-insensitive disk cannot
///   accept `24W14A` for an installed `24w14a` and store a spelling Mojang
///   never uses.
fn installed(versions_dir: &Path, mc: &str) -> bool {
    if crate::pathsafe::validate_segment(mc).is_err() {
        return false;
    }
    let named = match std::fs::read_dir(versions_dir) {
        Ok(entries) => entries
            .flatten()
            .any(|e| e.file_name().to_str() == Some(mc)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            crate::diag!(
                "instances: could not read {} to look for Minecraft {mc}: {e}",
                versions_dir.display()
            );
            false
        }
    };
    named && crate::versions::install::vanilla_installed_in(versions_dir, mc)
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

    /// `<dir>/<id>/<id>.json` and `.jar`, as an install leaves them.
    fn plant(dir: &Path, id: &str, jar: bool) {
        let folder = dir.join(id);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(format!("{id}.json")), "{}").unwrap();
        if jar {
            std::fs::write(folder.join(format!("{id}.jar")), "PK").unwrap();
        }
    }

    #[test]
    fn installed_means_the_json_and_the_client_jar() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!installed(dir.path(), "1.20.1"));
        // The JSON alone — an install that stopped after it, a repair scan, a
        // Forge installer's parent, a loader profile — is not an installed version.
        plant(dir.path(), "1.20.1", false);
        assert!(!installed(dir.path(), "1.20.1"));
        plant(dir.path(), "1.20.1", true);
        assert!(installed(dir.path(), "1.20.1"));
    }

    #[test]
    fn only_the_folder_s_own_spelling_counts() {
        let dir = tempfile::tempdir().unwrap();
        plant(dir.path(), "24w14a", true);
        assert!(installed(dir.path(), "24w14a"));
        // On a case-insensitive disk the files would be found under this spelling too.
        assert!(!installed(dir.path(), "24W14A"));
    }

    #[test]
    fn an_id_that_names_another_path_is_never_looked_up() {
        let dir = tempfile::tempdir().unwrap();
        let versions = dir.path().join("versions");
        std::fs::create_dir_all(&versions).unwrap();
        // Joined as paths these would reach planted installs: "../x" reads
        // `versions/../x/../x.json` (= `<dir>/x.json`), "a/b" and "a\\b" read
        // `versions/a/b/a/b.json` (on Windows for the backslash).
        plant(dir.path(), "x", true);
        std::fs::write(dir.path().join("x.json"), "{}").unwrap();
        std::fs::write(dir.path().join("x.jar"), "PK").unwrap();
        let ab = versions.join("a").join("b").join("a");
        std::fs::create_dir_all(&ab).unwrap();
        std::fs::write(ab.join("b.json"), "{}").unwrap();
        std::fs::write(ab.join("b.jar"), "PK").unwrap();
        for id in [
            "../x", "..\\x", "a/b", "a\\b", ".", "..", "", "C:x", "C:", "C:\\x", "\\x", "/etc",
            "NUL", "CON",
        ] {
            assert!(!installed(&versions, id), "{id:?} must not be looked up");
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

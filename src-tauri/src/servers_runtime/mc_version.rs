//! The one rule for a server's Minecraft version: it is given, and Mojang
//! lists it. Starting a server resolves its Java runtime through Mojang's
//! version manifest, so a version the manifest does not list can never start.
//!
//! Two sides of the rule. A NEW server (create, import) is checked at the IPC
//! boundary by [`check_new`], before anything touches disk. A SAVED server that
//! breaks it (imported before the import required a version) is named for what
//! it is by [`saved_recorded`] / [`saved_entry`], instead of being blamed on a
//! missing vanilla download.

use crate::error::{Error, Result};
use crate::versions::VersionEntry;

fn find<'a>(mc: &str, known: &'a [VersionEntry]) -> Option<&'a VersionEntry> {
    known.iter().find(|e| e.id == mc)
}

/// A new server's version, trimmed. Blank → `ServerMcVersionRequired`.
pub fn given(input: &str) -> Result<String> {
    let mc = input.trim();
    if mc.is_empty() {
        return Err(Error::ServerMcVersionRequired);
    }
    Ok(mc.to_string())
}

/// A new server's (trimmed) version must be an id in Mojang's manifest.
pub fn listed(mc: &str, known: &[VersionEntry]) -> Result<()> {
    match find(mc, known) {
        Some(_) => Ok(()),
        None => Err(Error::ServerMcVersionUnlisted {
            mc_version: mc.to_string(),
        }),
    }
}

/// `list_manifest` failed while a new server's version was being checked. A
/// transport failure (which also covers a Mojang-side error status or an
/// unreadable list) becomes `ServerMcVersionUnchecked` — the generic network
/// copy, "Couldn't reach the server", would read as the user's own Minecraft
/// server here — and its cause goes to lucerna.log. Any other kind already
/// names itself and passes through unchanged.
pub fn manifest_unreachable(e: Error) -> Error {
    match e {
        Error::Network { url, details } => {
            crate::diag!("servers: could not load Mojang's version list from {url}: {details}");
            Error::ServerMcVersionUnchecked
        }
        other => other,
    }
}

/// The boundary check for a new server: given, then listed by Mojang. A blank
/// value is refused before the network is needed; a list that cannot be loaded
/// refuses the server rather than saving an unchecked id. Returns the trimmed
/// id to store.
pub async fn check_new(input: &str) -> Result<String> {
    let mc = given(input)?;
    let known = crate::versions::manifest::list_manifest()
        .await
        .map_err(manifest_unreachable)?;
    listed(&mc, &known)?;
    Ok(mc)
}

/// A saved server's version, checked before any manifest lookup so a server
/// saved with a blank version says so even offline.
pub fn saved_recorded(mc: &str) -> Result<()> {
    if mc.trim().is_empty() {
        return Err(Error::ServerSavedMcVersionMissing);
    }
    Ok(())
}

/// The manifest entry for a saved server's version.
pub fn saved_entry<'a>(mc: &str, known: &'a [VersionEntry]) -> Result<&'a VersionEntry> {
    find(mc, known).ok_or_else(|| Error::ServerSavedMcVersionUnknown {
        mc_version: mc.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions::VersionType;

    fn entry(id: &str, version_type: VersionType) -> VersionEntry {
        VersionEntry {
            id: id.to_string(),
            version_type,
            release_date: String::new(),
            url: format!("https://example.invalid/{id}.json"),
        }
    }

    fn known() -> Vec<VersionEntry> {
        vec![
            entry("1.20.4", VersionType::Release),
            entry("24w14a", VersionType::Snapshot),
        ]
    }

    #[test]
    fn a_blank_new_version_is_required() {
        for blank in ["", "   ", "\t\n"] {
            assert!(matches!(given(blank), Err(Error::ServerMcVersionRequired)));
        }
    }

    #[test]
    fn a_new_version_is_trimmed() {
        assert_eq!(given("  1.20.4 ").unwrap(), "1.20.4");
    }

    #[test]
    fn a_listed_release_or_snapshot_passes() {
        assert!(listed("1.20.4", &known()).is_ok());
        assert!(listed("24w14a", &known()).is_ok());
    }

    #[test]
    fn an_unlisted_new_version_is_named_in_the_error() {
        match listed("1.20.l", &known()) {
            Err(Error::ServerMcVersionUnlisted { mc_version }) => assert_eq!(mc_version, "1.20.l"),
            other => panic!("expected ServerMcVersionUnlisted, got {other:?}"),
        }
    }

    #[test]
    fn only_a_transport_failure_becomes_unchecked() {
        let transport = Error::network("https://piston-meta.mojang.com/x.json", "timed out");
        assert!(matches!(
            manifest_unreachable(transport),
            Error::ServerMcVersionUnchecked
        ));
        let refused = Error::HostNotAllowed {
            url: "https://example.invalid".into(),
        };
        assert!(matches!(
            manifest_unreachable(refused),
            Error::HostNotAllowed { .. }
        ));
    }

    #[test]
    fn a_saved_blank_version_is_missing() {
        assert!(matches!(
            saved_recorded(""),
            Err(Error::ServerSavedMcVersionMissing)
        ));
        assert!(matches!(
            saved_recorded("  "),
            Err(Error::ServerSavedMcVersionMissing)
        ));
        assert!(saved_recorded("1.20.4").is_ok());
    }

    #[test]
    fn a_saved_version_resolves_to_its_manifest_entry() {
        let list = known();
        assert_eq!(saved_entry("1.20.4", &list).unwrap().id, "1.20.4");
        match saved_entry("1.20.l", &list) {
            Err(Error::ServerSavedMcVersionUnknown { mc_version }) => {
                assert_eq!(mc_version, "1.20.l")
            }
            other => panic!("expected ServerSavedMcVersionUnknown, got {other:?}"),
        }
    }
}

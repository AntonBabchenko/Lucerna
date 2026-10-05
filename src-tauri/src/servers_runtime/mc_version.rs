//! The one rule for a server's Minecraft version: it is given, and Mojang
//! lists it. Starting a server resolves its Java runtime through Mojang's
//! version manifest, so a version the manifest does not list can never start.
//!
//! Two sides of the rule. A NEW server (create, import) is checked at the IPC
//! boundary by [`check_new`], before anything touches disk — the shared rule of
//! `versions::check`, held strictly. A SAVED server that breaks it (imported
//! before the import required a version) is named for what it is by
//! [`saved_recorded`] / [`saved_entry`], instead of being blamed on a missing
//! vanilla download.

use crate::error::{Error, Result};
use crate::versions::check::{given, listed, manifest_unreachable};
use crate::versions::VersionEntry;

/// The boundary check for a new server: given, then listed by Mojang. A blank
/// value is refused before the network is needed; a list that cannot be loaded
/// refuses the server rather than saving an unchecked id — unlike an imported
/// profile, a server cannot start on an installed version without the list.
/// Returns the trimmed id to store.
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
    known
        .iter()
        .find(|e| e.id == mc)
        .ok_or_else(|| Error::ServerSavedMcVersionUnknown {
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

    // The rule for a NEW version (given, listed, unchecked) is tested where it
    // lives, in `versions::check`.

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

//! The rule for a Minecraft version a user gives for something new — a server
//! (create, import) or a profile imported from another launcher: it is given,
//! and Mojang lists it. Lucerna installs every version through Mojang's version
//! manifest, so an id the manifest does not list can never be installed.
//!
//! The pieces are composed by each caller: a server checks the list strictly
//! (`servers_runtime::mc_version::check_new`); an imported profile also accepts
//! a version already installed on this machine when the list cannot be loaded
//! (`instances::import::mc_version`).

use crate::error::{Error, Result};
use crate::versions::VersionEntry;

/// The given version, trimmed. Blank → `McVersionRequired`.
pub fn given(input: &str) -> Result<String> {
    let mc = input.trim();
    if mc.is_empty() {
        return Err(Error::McVersionRequired);
    }
    Ok(mc.to_string())
}

/// The (trimmed) version must be an id in Mojang's manifest.
pub fn listed(mc: &str, known: &[VersionEntry]) -> Result<()> {
    if known.iter().any(|e| e.id == mc) {
        Ok(())
    } else {
        Err(Error::McVersionUnlisted {
            mc_version: mc.to_string(),
        })
    }
}

/// `list_manifest` failed while a version was being checked. A transport
/// failure (which also covers a Mojang-side error status or an unreadable list)
/// becomes `McVersionUnchecked` — the generic network copy, "Couldn't reach the
/// server", would read as the user's own Minecraft server — and its cause goes
/// to lucerna.log. Any other kind already names itself and passes through
/// unchanged.
pub fn manifest_unreachable(e: Error) -> Error {
    match e {
        Error::Network { url, details } => {
            crate::diag!("versions: could not load Mojang's version list from {url}: {details}");
            Error::McVersionUnchecked
        }
        other => other,
    }
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
            assert!(matches!(given(blank), Err(Error::McVersionRequired)));
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
            Err(Error::McVersionUnlisted { mc_version }) => assert_eq!(mc_version, "1.20.l"),
            other => panic!("expected McVersionUnlisted, got {other:?}"),
        }
    }

    #[test]
    fn only_a_transport_failure_becomes_unchecked() {
        let transport = Error::network("https://piston-meta.mojang.com/x.json", "timed out");
        assert!(matches!(
            manifest_unreachable(transport),
            Error::McVersionUnchecked
        ));
        let refused = Error::HostNotAllowed {
            url: "https://example.invalid".into(),
        };
        assert!(matches!(
            manifest_unreachable(refused),
            Error::HostNotAllowed { .. }
        ));
    }
}

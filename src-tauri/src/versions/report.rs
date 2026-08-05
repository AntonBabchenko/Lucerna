//! Pure builders for the game-install report rows.
//!
//! I/O-free on purpose. `install_version` owns an `AppHandle`, which cannot be
//! constructed in a unit test, so every decision that could be wrong — which
//! outcome a file gets, what a phase aggregate looks like — lives here and is
//! tested directly rather than through the pipeline.

use crate::mods::store::Placement;
use crate::tasks::{DetailOutcome, Fetched, TaskDetail, TaskOrigin};

/// The outcome every game-install row uses when bytes really moved.
///
/// `Copied`, not `Linked`: game files are written straight to their
/// destination by `download_with_sha`, never hardlinked out of the shared
/// content cache the way mod jars are. (`fetched` and `placement` are
/// orthogonal — see [`DetailOutcome`] — so this is a real statement about
/// placement, not a restatement of "was downloaded".)
fn fetched_outcome() -> DetailOutcome {
    DetailOutcome::Installed {
        fetched: Fetched::Downloaded,
        placement: Placement::Copied,
    }
}

/// One row for a file the install pipeline considered.
///
/// `downloaded == false` means the SHA precheck matched and nothing was
/// fetched — `Unchanged`, the same word the modpack paths use for a
/// byte-identical destination. Re-installing a complete instance therefore
/// produces an honest all-unchanged report instead of claiming work it did
/// not do.
///
/// A zero `bytes` becomes `None`, not `Some(0)`: Fabric/Quilt maven libraries
/// carry no size in their metadata, and "0 B" in the report would read as an
/// empty file rather than an unknown size. Same for an empty `sha1`, which is
/// what those same libraries (and locally-produced Forge artifacts) have.
pub(crate) fn file_row(
    name: impl Into<String>,
    install_path: impl Into<String>,
    url: &str,
    sha1: &str,
    bytes: Option<u64>,
    downloaded: bool,
) -> TaskDetail {
    TaskDetail {
        name: name.into(),
        install_path: install_path.into(),
        origin: TaskOrigin::Game,
        host: crate::network::request::host_of(url),
        bytes: bytes.filter(|b| *b > 0).map(|b| b as f64),
        sha1: (!sha1.is_empty()).then(|| sha1.to_string()),
        outcome: if downloaded {
            fetched_outcome()
        } else {
            DetailOutcome::Unchanged
        },
    }
}

/// The single row that stands in for a whole phase of content-addressed files.
///
/// Assets and the JRE get one of these instead of per-file rows: one MC
/// version carries 2000-4000 asset objects, and a report that long is both
/// unreadable and megabytes of retained state per finished task (the frontend
/// registry keeps reports for the ten most recent).
///
/// `name` is the phase's on-disk directory, not a phrase like "Assets (2431
/// files)". `TaskDetail::name` is an artefact identifier at every other
/// producer and the report modal renders it raw, so UI copy here would be
/// untranslatable text on a localized surface.
///
/// `transferred_bytes` is what this run actually pulled, so zero means the
/// phase was already complete — exactly the `Unchanged` condition. Nothing
/// here takes a file count, which is deliberate: the JRE's fully-cached fast
/// path reports a placeholder total of 1, and a row that showed counts would
/// publish that fiction.
pub(crate) fn phase_row(
    name: impl Into<String>,
    install_path: impl Into<String>,
    host: Option<String>,
    transferred_bytes: u64,
) -> TaskDetail {
    TaskDetail {
        name: name.into(),
        install_path: install_path.into(),
        origin: TaskOrigin::Game,
        host,
        bytes: (transferred_bytes > 0).then_some(transferred_bytes as f64),
        sha1: None,
        outcome: if transferred_bytes > 0 {
            fetched_outcome()
        } else {
            DetailOutcome::Unchanged
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fetched_file_is_installed_downloaded_and_copied() {
        let row = file_row(
            "client.jar",
            "versions/1.20.4/1.20.4.jar",
            "https://piston-data.mojang.com/v1/objects/abc/client.jar",
            "abc123",
            Some(25_000_000),
            true,
        );
        assert_eq!(
            row.outcome,
            DetailOutcome::Installed {
                fetched: Fetched::Downloaded,
                placement: Placement::Copied,
            }
        );
        assert_eq!(row.origin, TaskOrigin::Game);
        assert_eq!(row.host.as_deref(), Some("piston-data.mojang.com"));
        assert_eq!(row.sha1.as_deref(), Some("abc123"));
        assert_eq!(row.bytes, Some(25_000_000.0));
    }

    #[test]
    fn a_precheck_hit_is_unchanged_but_keeps_its_size() {
        let row = file_row(
            "client.jar",
            "versions/1.20.4/1.20.4.jar",
            "https://piston-data.mojang.com/v1/objects/abc/client.jar",
            "abc123",
            Some(25_000_000),
            false,
        );
        assert_eq!(row.outcome, DetailOutcome::Unchanged);
        // The file IS that big; it just wasn't fetched this run.
        assert_eq!(row.bytes, Some(25_000_000.0));
    }

    #[test]
    fn a_locally_produced_library_has_no_host_and_no_sha() {
        // Modern Forge's `{PATCHED}` client jar: empty URL, and version.json's
        // published SHA1 refers to the reference installer's output, which our
        // local binarypatcher cannot reproduce bytewise. Trusted by existence.
        let row = file_row(
            "forge-1.20.4-49.0.49-client.jar",
            "libraries/net/minecraftforge/forge/x/forge-client.jar",
            "",
            "",
            None,
            false,
        );
        assert_eq!(row.host, None);
        assert_eq!(row.sha1, None);
        assert_eq!(row.bytes, None);
        assert_eq!(row.outcome, DetailOutcome::Unchanged);
    }

    #[test]
    fn a_zero_size_is_reported_as_unknown_not_as_zero_bytes() {
        let row = file_row(
            "fabric-loader-0.15.7.jar",
            "libraries/net/fabricmc/fabric-loader/0.15.7/fabric-loader-0.15.7.jar",
            "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.15.7/fabric-loader-0.15.7.jar",
            "",
            Some(0),
            true,
        );
        assert_eq!(row.bytes, None);
        assert_eq!(row.sha1, None);
        assert_eq!(row.host.as_deref(), Some("maven.fabricmc.net"));
    }

    #[test]
    fn a_phase_that_transferred_nothing_is_unchanged_with_no_size() {
        let row = phase_row(
            "assets/objects",
            "assets/objects",
            Some("resources.download.minecraft.net".into()),
            0,
        );
        assert_eq!(row.outcome, DetailOutcome::Unchanged);
        assert_eq!(row.bytes, None);
        assert_eq!(row.sha1, None);
    }

    #[test]
    fn a_phase_that_transferred_bytes_is_installed_with_that_size() {
        let row = phase_row(
            "assets/objects",
            "assets/objects",
            Some("resources.download.minecraft.net".into()),
            42_400_000,
        );
        assert_eq!(
            row.outcome,
            DetailOutcome::Installed {
                fetched: Fetched::Downloaded,
                placement: Placement::Copied,
            }
        );
        assert_eq!(row.bytes, Some(42_400_000.0));
        assert_eq!(row.origin, TaskOrigin::Game);
    }
}

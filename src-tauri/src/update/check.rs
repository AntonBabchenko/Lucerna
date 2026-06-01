//! Fetch the latest GitHub release, compare versions, and select the
//! installer / SHA256SUMS / cosign-bundle assets.

use crate::error::{Error, Result};
use crate::update::{ReleaseAsset, UpdateInfo};
use serde::Deserialize;

/// `releases/latest` already excludes prereleases (the `-rc` tags), so
/// no extra filtering is needed here.
pub const RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/AntonBabchenko/Lucerna/releases/latest";

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

impl From<&GhAsset> for ReleaseAsset {
    fn from(a: &GhAsset) -> Self {
        ReleaseAsset {
            name: a.name.clone(),
            url: a.browser_download_url.clone(),
            size: a.size as f64,
        }
    }
}

fn parse_semver(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// True iff `latest` is a strictly higher semver than `current`.
/// Malformed input on either side → false (never offer an update on
/// something we cannot parse).
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_semver(latest), parse_semver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// True only for a bare filename. Asset names come from the GitHub API
/// and are later joined onto the updates dir, so a name with a path
/// separator, `..`, a drive letter, or other path syntax could escape it
/// (Windows `Path::join` with an absolute path discards the base).
/// Defense-in-depth: a crafted name cannot pass cosign anyway, but we
/// never let an attacker-influenced name steer a filesystem path.
fn is_bare_filename(name: &str) -> bool {
    !name.is_empty()
        && name != ".."
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
}

fn build_update_info(rel: GhRelease, current: &str) -> Result<UpdateInfo> {
    let latest = rel
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&rel.tag_name)
        .to_string();

    let installer = rel
        .assets
        .iter()
        .find(|a| a.name.ends_with("-setup.exe") && !a.name.ends_with(".cosign.bundle"))
        .ok_or_else(|| Error::UpdateCheckFailed {
            details: "release has no *-setup.exe asset".into(),
        })?;
    // The installer name steers the download path (and the bundle name is
    // derived from it); SHA256SUMS is a constant. Reject anything that is
    // not a bare filename before it reaches `dir.join`.
    if !is_bare_filename(&installer.name) {
        return Err(Error::UpdateCheckFailed {
            details: format!("unsafe installer asset name: {}", installer.name),
        });
    }
    let sha256sums = rel
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS")
        .ok_or_else(|| Error::UpdateCheckFailed {
            details: "release has no SHA256SUMS asset".into(),
        })?;
    let bundle_name = format!("{}.cosign.bundle", installer.name);
    let cosign_bundle = rel
        .assets
        .iter()
        .find(|a| a.name == bundle_name)
        .ok_or_else(|| Error::UpdateCheckFailed {
            details: format!("release has no {bundle_name} asset"),
        })?;

    Ok(UpdateInfo {
        available: is_newer(&latest, current),
        current: current.to_string(),
        latest,
        release_url: rel.html_url,
        installer: installer.into(),
        sha256sums: sha256sums.into(),
        cosign_bundle: cosign_bundle.into(),
    })
}

/// Fetch the latest release and build an `UpdateInfo` relative to
/// `current` (the running version). Network failures and unusable
/// release shapes both return `Err` — the caller decides whether to
/// surface them (the startup path swallows them silently).
pub async fn check_for_update(current: &str) -> Result<UpdateInfo> {
    let rel: GhRelease = crate::network::get_json(RELEASES_LATEST_URL, "update").await?;
    build_update_info(rel, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_compares_semver() {
        assert!(is_newer("0.9.1", "0.9.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.9.0", "0.9.0"));
        assert!(!is_newer("0.8.9", "0.9.0"));
    }

    #[test]
    fn is_newer_rejects_malformed() {
        assert!(!is_newer("garbage", "0.9.0"));
        assert!(!is_newer("0.9", "0.9.0"));
        assert!(!is_newer("0.9.0", "also-bad"));
        assert!(!is_newer("0.9.0.1", "0.9.0"));
    }

    fn sample_release() -> GhRelease {
        GhRelease {
            tag_name: "v0.9.1".into(),
            html_url: "https://github.com/AntonBabchenko/Lucerna/releases/tag/v0.9.1".into(),
            assets: vec![
                GhAsset {
                    name: "Lucerna_0.9.1_x64-setup.exe".into(),
                    browser_download_url: "https://github.com/dl/setup".into(),
                    size: 12,
                },
                GhAsset {
                    name: "Lucerna_0.9.1_x64-setup.exe.cosign.bundle".into(),
                    browser_download_url: "https://github.com/dl/bundle".into(),
                    size: 3,
                },
                GhAsset {
                    name: "SHA256SUMS".into(),
                    browser_download_url: "https://github.com/dl/sums".into(),
                    size: 1,
                },
                GhAsset {
                    name: "lucerna-0.9.1.cdx.json".into(),
                    browser_download_url: "https://github.com/dl/sbom".into(),
                    size: 5,
                },
            ],
        }
    }

    #[test]
    fn build_update_info_selects_assets_and_strips_v() {
        let info = build_update_info(sample_release(), "0.9.0").unwrap();
        assert_eq!(info.latest, "0.9.1");
        assert!(info.available);
        assert_eq!(info.installer.name, "Lucerna_0.9.1_x64-setup.exe");
        assert_eq!(
            info.cosign_bundle.name,
            "Lucerna_0.9.1_x64-setup.exe.cosign.bundle"
        );
        assert_eq!(info.sha256sums.name, "SHA256SUMS");
        assert_eq!(info.installer.url, "https://github.com/dl/setup");
    }

    #[test]
    fn build_update_info_same_version_not_available() {
        let info = build_update_info(sample_release(), "0.9.1").unwrap();
        assert!(!info.available);
    }

    #[test]
    fn build_update_info_rejects_path_traversal_installer_name() {
        let mut rel = sample_release();
        rel.assets[0].name = "..\\..\\evil-setup.exe".into();
        rel.assets[1].name = "..\\..\\evil-setup.exe.cosign.bundle".into();
        let r = build_update_info(rel, "0.9.0");
        assert!(matches!(
            r,
            Err(crate::error::Error::UpdateCheckFailed { .. })
        ));
    }

    #[test]
    fn is_bare_filename_accepts_real_installer_rejects_paths() {
        assert!(is_bare_filename("Lucerna_0.9.1_x64-setup.exe"));
        assert!(!is_bare_filename("../evil.exe"));
        assert!(!is_bare_filename("..\\evil.exe"));
        assert!(!is_bare_filename("C:\\evil.exe"));
        assert!(!is_bare_filename(".."));
        assert!(!is_bare_filename(""));
    }

    #[test]
    fn build_update_info_missing_installer_errors() {
        let mut rel = sample_release();
        rel.assets
            .retain(|a| !a.name.ends_with("-setup.exe") || a.name.ends_with(".cosign.bundle"));
        let r = build_update_info(rel, "0.9.0");
        assert!(matches!(
            r,
            Err(crate::error::Error::UpdateCheckFailed { .. })
        ));
    }
}

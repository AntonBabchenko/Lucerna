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
    let available = is_newer(&latest, current);
    let release_url = rel.html_url.clone();

    // Install assets are only needed where we do in-app install, and the
    // primary artifact differs by mechanism (Windows `-setup.exe` vs Linux
    // `.AppImage`). Notify-only platforms never download, so we don't require
    // any of them to exist — the UI links to the release page instead.
    let (installer, sha256sums, cosign_bundle) = match primary_asset_suffix() {
        Some(suffix) => {
            let (i, s, b) = select_install_assets(&rel, suffix)?;
            (Some(i), Some(s), Some(b))
        }
        None => (None, None, None),
    };

    Ok(UpdateInfo {
        available,
        current: current.to_string(),
        latest,
        release_url,
        installer,
        sha256sums,
        cosign_bundle,
    })
}

/// Filename suffix of the primary install asset for this run's mechanism, or
/// `None` when notify-only (no in-app install).
fn primary_asset_suffix() -> Option<&'static str> {
    match crate::platform::install_kind() {
        crate::platform::InstallKind::WindowsInstaller => Some("-setup.exe"),
        crate::platform::InstallKind::LinuxAppImage { .. } => Some(".AppImage"),
        crate::platform::InstallKind::NotifyOnly => None,
    }
}

/// Find and validate the in-app-install assets: the primary artifact (matched
/// by `suffix` — the NSIS installer on Windows, the AppImage on Linux), its
/// cosign bundle, and SHA256SUMS. Returns `Err(UpdateCheckFailed)` if any is
/// missing or the primary asset name is not a bare filename.
fn select_install_assets(
    rel: &GhRelease,
    suffix: &str,
) -> Result<(ReleaseAsset, ReleaseAsset, ReleaseAsset)> {
    let installer = rel
        .assets
        .iter()
        .find(|a| a.name.ends_with(suffix) && !a.name.ends_with(".cosign.bundle"))
        .ok_or_else(|| Error::UpdateCheckFailed {
            details: format!("release has no *{suffix} asset"),
        })?;
    // The installer name steers the download path (and the bundle name is
    // derived from it); SHA256SUMS is a constant. Reject anything that is
    // not a bare filename before it reaches `dir.join` — a crafted name
    // (path separator, `..`, drive letter) could otherwise escape the
    // updates dir. Defense-in-depth: a bad name can't pass cosign anyway.
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
    Ok((installer.into(), sha256sums.into(), cosign_bundle.into()))
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
    fn build_update_info_reports_version_and_url_on_all_platforms() {
        let info = build_update_info(sample_release(), "0.9.0").unwrap();
        assert_eq!(info.latest, "0.9.1");
        assert!(info.available);
        assert_eq!(
            info.release_url,
            "https://github.com/AntonBabchenko/Lucerna/releases/tag/v0.9.1"
        );
    }

    #[cfg(windows)]
    #[test]
    fn build_update_info_selects_install_assets_on_windows() {
        let info = build_update_info(sample_release(), "0.9.0").unwrap();
        let installer = info.installer.expect("windows populates installer");
        assert_eq!(installer.name, "Lucerna_0.9.1_x64-setup.exe");
        assert_eq!(installer.url, "https://github.com/dl/setup");
        assert_eq!(
            info.cosign_bundle.unwrap().name,
            "Lucerna_0.9.1_x64-setup.exe.cosign.bundle"
        );
        assert_eq!(info.sha256sums.unwrap().name, "SHA256SUMS");
    }

    #[cfg(not(windows))]
    #[test]
    fn build_update_info_notify_only_leaves_assets_none() {
        let info = build_update_info(sample_release(), "0.9.0").unwrap();
        assert!(info.available, "notify-only still flags an update");
        assert!(info.installer.is_none());
        assert!(info.sha256sums.is_none());
        assert!(info.cosign_bundle.is_none());
    }

    #[test]
    fn build_update_info_same_version_not_available() {
        let info = build_update_info(sample_release(), "0.9.1").unwrap();
        assert!(!info.available);
    }

    #[cfg(windows)]
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

    #[cfg(windows)]
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

    fn sample_release_appimage() -> GhRelease {
        GhRelease {
            tag_name: "v0.9.1".into(),
            html_url: "https://github.com/AntonBabchenko/Lucerna/releases/tag/v0.9.1".into(),
            assets: vec![
                GhAsset {
                    name: "Lucerna_0.9.1_amd64.AppImage".into(),
                    browser_download_url: "https://github.com/dl/appimage".into(),
                    size: 50,
                },
                GhAsset {
                    name: "Lucerna_0.9.1_amd64.AppImage.cosign.bundle".into(),
                    browser_download_url: "https://github.com/dl/appimage-bundle".into(),
                    size: 3,
                },
                GhAsset {
                    name: "SHA256SUMS".into(),
                    browser_download_url: "https://github.com/dl/sums".into(),
                    size: 1,
                },
            ],
        }
    }

    // Asset selection is pure string matching, so it is exercised directly with
    // an explicit suffix on every platform (the `build_update_info` paths above
    // only reach it under the matching cfg).
    #[test]
    fn select_install_assets_picks_appimage_and_derives_bundle() {
        let rel = sample_release_appimage();
        let (installer, sums, bundle) = select_install_assets(&rel, ".AppImage").unwrap();
        assert_eq!(installer.name, "Lucerna_0.9.1_amd64.AppImage");
        assert_eq!(installer.url, "https://github.com/dl/appimage");
        assert_eq!(bundle.name, "Lucerna_0.9.1_amd64.AppImage.cosign.bundle");
        assert_eq!(sums.name, "SHA256SUMS");
    }

    #[test]
    fn select_install_assets_appimage_missing_errors() {
        let mut rel = sample_release_appimage();
        rel.assets
            .retain(|a| !a.name.ends_with(".AppImage") || a.name.ends_with(".cosign.bundle"));
        assert!(matches!(
            select_install_assets(&rel, ".AppImage"),
            Err(Error::UpdateCheckFailed { .. })
        ));
    }

    #[test]
    fn select_install_assets_setup_exe_excludes_its_bundle() {
        let (installer, _, bundle) =
            select_install_assets(&sample_release(), "-setup.exe").unwrap();
        assert_eq!(installer.name, "Lucerna_0.9.1_x64-setup.exe");
        assert_eq!(bundle.name, "Lucerna_0.9.1_x64-setup.exe.cosign.bundle");
    }
}

//! Download the official installer + cosign bundle + SHA256SUMS,
//! verify (SHA-256 then cosign), launch the installer, and exit so it
//! can replace the locked launcher binary. Always user-initiated.

use crate::error::{Error, Result};
use crate::update::{verify, UpdateInfo};

/// Download to the update scratch dir, verify, launch, and exit.
/// On any download/verify failure returns `Err` WITHOUT launching —
/// an unverified binary is never run.
pub async fn download_and_install(app: &tauri::AppHandle, info: &UpdateInfo) -> Result<()> {
    let dir = crate::paths::update_dir(app).map_err(|e| Error::UpdateInstallFailed {
        details: format!("update dir: {e}"),
    })?;
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir.display().to_string(), e))?;

    let installer_path = dir.join(&info.installer.name);
    let bundle_path = dir.join(&info.cosign_bundle.name);

    // Download installer + bundle. Pass "" to skip the streaming SHA-1
    // check (that primitive verifies SHA-1; our SHA-256 + cosign run
    // afterwards). browser_download_url host is github.com (allowlisted);
    // the CDN redirect is followed by the shared client.
    crate::network::download::download_with_sha(
        app,
        &info.installer.url,
        &installer_path,
        "",
        "update",
    )
    .await?;
    crate::network::download::download_with_sha(
        app,
        &info.cosign_bundle.url,
        &bundle_path,
        "",
        "update",
    )
    .await?;

    let sums = crate::network::get_text(&info.sha256sums.url, "update").await?;

    verify::verify_sha256(&installer_path, &info.installer.name, &sums)?;

    // cosign verify is synchronous + CPU/IO-heavy (reads the whole
    // installer + crypto) — run it off the async runtime thread. The
    // version pins the exact signing-identity SAN.
    let ip = installer_path.clone();
    let bp = bundle_path.clone();
    let ver = info.latest.clone();
    tokio::task::spawn_blocking(move || verify::verify_cosign(&ip, &bp, &ver))
        .await
        .map_err(|e| Error::UpdateVerificationFailed {
            details: format!("verify task: {e}"),
        })??;

    crate::process::spawn_installer(&installer_path)?;
    app.exit(0);
    Ok(())
}

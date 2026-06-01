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
    // Start each attempt from an empty dir so installers/bundles from
    // previous versions don't accumulate (the dir holds only the binary
    // currently being verified + launched). Ignore "not found".
    if let Err(e) = tokio::fs::remove_dir_all(&dir).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(Error::io(dir.display().to_string(), e));
        }
    }
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| Error::io(dir.display().to_string(), e))?;

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

    // Verify off the async runtime thread: read the (tens-of-MB) installer
    // ONCE and run both layers over the same bytes — no double read, and no
    // TOCTOU between them. SHA-256 first (cheap reject on a corrupt
    // download), then cosign. `spawn_installer` is reached only if BOTH pass.
    let ip = installer_path.clone();
    let bp = bundle_path.clone();
    let name = info.installer.name.clone();
    let ver = info.latest.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let bytes = std::fs::read(&ip).map_err(|e| Error::io(ip.display().to_string(), e))?;
        verify::verify_sha256(&bytes, &name, &sums)?;
        verify::verify_cosign(&bytes, &bp, &ver)
    })
    .await
    .map_err(|e| Error::UpdateVerificationFailed {
        details: format!("verify task: {e}"),
    })??;

    crate::process::spawn_installer(&installer_path)?;
    app.exit(0);
    Ok(())
}

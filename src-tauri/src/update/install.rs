//! Download the official installer + cosign bundle + SHA256SUMS,
//! verify (SHA-256 then cosign), launch the installer, and exit so it
//! can replace the locked launcher binary. Always user-initiated.

use crate::error::{Error, Result};
use crate::update::{verify, UpdateInfo};

/// Download to the update scratch dir, verify, launch, and exit.
/// On any download/verify failure returns `Err` WITHOUT launching —
/// an unverified binary is never run.
pub async fn download_and_install(app: &tauri::AppHandle, info: &UpdateInfo) -> Result<()> {
    let installer = info
        .installer
        .as_ref()
        .ok_or_else(|| Error::UpdateInstallFailed {
            details: "in-app install is not supported on this platform".into(),
        })?;
    let cosign_bundle = info
        .cosign_bundle
        .as_ref()
        .ok_or_else(|| Error::UpdateInstallFailed {
            details: "release has no cosign bundle for in-app install".into(),
        })?;
    let sha256sums = info
        .sha256sums
        .as_ref()
        .ok_or_else(|| Error::UpdateInstallFailed {
            details: "release has no SHA256SUMS for in-app install".into(),
        })?;

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

    let installer_path = dir.join(&installer.name);
    let bundle_path = dir.join(&cosign_bundle.name);

    // Download installer + bundle. Pass "" to skip the streaming SHA-1
    // check (that primitive verifies SHA-1; our SHA-256 + cosign run
    // afterwards). browser_download_url host is github.com (allowlisted);
    // the CDN redirect is followed by the shared client.
    crate::network::download::download_with_sha(app, &installer.url, &installer_path, "", "update")
        .await?;
    crate::network::download::download_with_sha(
        app,
        &cosign_bundle.url,
        &bundle_path,
        "",
        "update",
    )
    .await?;

    let sums = crate::network::get_text(&sha256sums.url, "update").await?;

    // Verify off the async runtime thread: read the (tens-of-MB) installer
    // ONCE and run both layers over the same bytes — no double read, and no
    // TOCTOU between them. SHA-256 first (cheap reject on a corrupt
    // download), then cosign. `spawn_installer` is reached only if BOTH pass.
    let ip = installer_path.clone();
    let bp = bundle_path.clone();
    let name = installer.name.clone();
    let ver = info.latest.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        verify_and_launch(
            &ip,
            |bytes| {
                verify::verify_sha256(bytes, &name, &sums)?;
                verify::verify_cosign(bytes, &bp, &ver)
            },
            crate::process::spawn_installer,
        )
    })
    .await
    .map_err(|e| Error::UpdateInstallFailed {
        details: format!("install task: {e}"),
    })??;

    app.exit(0);
    Ok(())
}

/// Read the installer at `installer_path`, verify it, and only then launch it.
///
/// The security guarantee lives here: `launch` is reached **iff** `verify`
/// returns `Ok`. A read error or a verification failure short-circuits via `?`
/// before `launch` is ever called, so an unverified binary is never run. Both
/// steps are taken as closures so the ordering can be tested without real
/// network I/O, a cosign bundle, or an actual installer process.
fn verify_and_launch(
    installer_path: &std::path::Path,
    verify: impl FnOnce(&[u8]) -> Result<()>,
    launch: impl FnOnce(&std::path::Path) -> Result<()>,
) -> Result<()> {
    let bytes = std::fs::read(installer_path)
        .map_err(|e| Error::io(installer_path.display().to_string(), e))?;
    verify(&bytes)?;
    launch(installer_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::io::Write;
    use std::path::Path;

    fn write_temp_installer() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("installer.exe");
        let mut f = std::fs::File::create(&path).expect("create installer");
        f.write_all(b"installer-bytes").expect("write installer");
        (dir, path)
    }

    #[test]
    fn verify_failure_does_not_launch() {
        let (_dir, path) = write_temp_installer();
        let launched = Cell::new(false);

        let result = verify_and_launch(
            &path,
            |_bytes| {
                Err(Error::UpdateVerificationFailed {
                    details: "forced failure".into(),
                })
            },
            |_p| {
                launched.set(true);
                Ok(())
            },
        );

        assert!(result.is_err(), "verify failure must propagate as Err");
        assert!(
            !launched.get(),
            "launch must NOT run when verification fails — unverified binary never launched",
        );
    }

    #[test]
    fn verify_success_launches_with_installer_path() {
        let (_dir, path) = write_temp_installer();
        let launched_with: Cell<Option<std::path::PathBuf>> = Cell::new(None);

        let result = verify_and_launch(
            &path,
            |bytes| {
                assert_eq!(bytes, b"installer-bytes", "verify sees the installer bytes");
                Ok(())
            },
            |p| {
                launched_with.set(Some(p.to_path_buf()));
                Ok(())
            },
        );

        assert!(result.is_ok(), "successful verify+launch returns Ok");
        assert_eq!(
            launched_with.into_inner().as_deref(),
            Some(path.as_path()),
            "launch runs exactly once with the installer path",
        );
    }

    #[test]
    fn launch_failure_propagates_as_err() {
        let (_dir, path) = write_temp_installer();

        let result = verify_and_launch(
            &path,
            |_bytes| Ok(()),
            |_p| {
                Err(Error::UpdateInstallFailed {
                    details: "forced launch failure".into(),
                })
            },
        );

        assert!(result.is_err(), "a launch failure must propagate as Err");
    }

    #[test]
    fn unreadable_installer_does_not_launch() {
        let launched = Cell::new(false);
        let missing = Path::new("definitely-not-a-real-installer-xyz.exe");

        let result = verify_and_launch(
            missing,
            |_bytes| Ok(()),
            |_p| {
                launched.set(true);
                Ok(())
            },
        );

        assert!(result.is_err(), "unreadable installer must error");
        assert!(
            !launched.get(),
            "launch must NOT run when the installer cannot be read",
        );
    }
}

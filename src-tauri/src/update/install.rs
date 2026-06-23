//! Download the official artifact + cosign bundle + SHA256SUMS, verify
//! (SHA-256 then cosign), apply, and exit. The apply step is per-mechanism:
//! Windows launches the NSIS installer so it can replace the locked binary; a
//! Linux AppImage swaps the running file in place and relaunches. Always
//! user-initiated.

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

    // Verify off the async runtime thread: read the (tens-of-MB) artifact
    // ONCE and run both layers over the same bytes — no double read, and no
    // TOCTOU between them. SHA-256 first (cheap reject on a corrupt
    // download), then cosign. The apply step is reached only if BOTH pass.
    let ip = installer_path.clone();
    let bp = bundle_path.clone();
    let name = installer.name.clone();
    let ver = info.latest.clone();

    // Verification is identical for every mechanism; only the apply differs.
    // Windows runs the NSIS installer; a Linux AppImage replaces itself in
    // place and relaunches. `verify_and_launch` guarantees the apply runs only
    // after BOTH checks pass.
    let launch: Box<dyn FnOnce(&std::path::Path) -> Result<()> + Send> =
        match crate::platform::install_kind() {
            crate::platform::InstallKind::WindowsInstaller => {
                Box::new(|p: &std::path::Path| crate::process::spawn_installer(p))
            }
            crate::platform::InstallKind::LinuxAppImage { path } => {
                Box::new(move |verified: &std::path::Path| apply_appimage_update(&path, verified))
            }
            crate::platform::InstallKind::NotifyOnly => {
                return Err(Error::UpdateInstallFailed {
                    details: "in-app install is not supported on this platform".into(),
                });
            }
        };

    tokio::task::spawn_blocking(move || -> Result<()> {
        verify_and_launch(
            &ip,
            |bytes| {
                verify::verify_sha256(bytes, &name, &sums)?;
                verify::verify_cosign(bytes, &bp, &ver)
            },
            launch,
        )
    })
    .await
    .map_err(|e| Error::UpdateInstallFailed {
        details: format!("install task: {e}"),
    })??;

    app.exit(0);
    Ok(())
}

/// Apply a verified AppImage update: swap the running file at `current` for the
/// downloaded `new_file`, then relaunch. Split into a pure file swap
/// (`install_appimage_over`, unit-tested) and the relaunch spawn.
fn apply_appimage_update(current: &std::path::Path, new_file: &std::path::Path) -> Result<()> {
    install_appimage_over(current, new_file)?;
    crate::process::spawn_appimage_relaunch(current)
}

/// Copy `new_file` over the running AppImage at `current` and mark it
/// executable. The bytes are staged in `current`'s OWN directory first so the
/// final `rename` is atomic on a single filesystem — the download scratch dir
/// may sit on a different mount, where a direct rename would fail (EXDEV). The
/// staging file is removed if any step fails, so a partial attempt never leaves
/// a stray `.part` next to the user's AppImage. Pure filesystem work (no
/// process spawn), so it is unit-testable.
fn install_appimage_over(current: &std::path::Path, new_file: &std::path::Path) -> Result<()> {
    let dir = current.parent().ok_or_else(|| Error::UpdateInstallFailed {
        details: format!("AppImage path has no parent dir: {}", current.display()),
    })?;
    let staged = dir.join(".lucerna-update.AppImage.part");
    let result = (|| -> Result<()> {
        std::fs::copy(new_file, &staged).map_err(|e| Error::io(staged.display().to_string(), e))?;
        crate::platform::set_executable(&staged)
            .map_err(|e| Error::io(staged.display().to_string(), e))?;
        // rename consumes `staged` on success; on failure it stays for cleanup.
        std::fs::rename(&staged, current).map_err(|e| Error::io(current.display().to_string(), e))
    })();
    if result.is_err() {
        // Best-effort: a leftover staging file would otherwise sit in the
        // user's application directory. Ignore the removal error.
        let _ = std::fs::remove_file(&staged);
    }
    result
}

/// Read the artifact at `artifact_path`, verify it, and only then apply it.
///
/// The security guarantee lives here: `apply` is reached **iff** `verify`
/// returns `Ok`. A read error or a verification failure short-circuits via `?`
/// before `apply` is ever called, so an unverified artifact is never used.
///
/// `apply` receives **the same `artifact_path`** whose bytes were just
/// verified, so it MUST act on that exact file — run it (Windows installer) or
/// copy it over the running binary (AppImage). It must not re-download or
/// substitute a different path, or the integrity check would not cover the
/// bytes that get applied. Both steps are closures so the ordering can be
/// tested without real network I/O, a cosign bundle, or a real process.
fn verify_and_launch(
    artifact_path: &std::path::Path,
    verify: impl FnOnce(&[u8]) -> Result<()>,
    apply: impl FnOnce(&std::path::Path) -> Result<()>,
) -> Result<()> {
    let bytes = std::fs::read(artifact_path)
        .map_err(|e| Error::io(artifact_path.display().to_string(), e))?;
    verify(&bytes)?;
    apply(artifact_path)
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

    #[test]
    fn install_appimage_over_replaces_contents_from_another_dir() {
        // Model the download scratch dir as a SEPARATE temp dir from the
        // AppImage's own dir, exercising the stage-then-rename (cross-dir) path.
        let live = tempfile::tempdir().unwrap();
        let current = live.path().join("Lucerna.AppImage");
        std::fs::write(&current, b"OLD VERSION").unwrap();

        let scratch = tempfile::tempdir().unwrap();
        let new_file = scratch.path().join("downloaded.AppImage");
        std::fs::write(&new_file, b"NEW VERSION").unwrap();

        install_appimage_over(&current, &new_file).expect("swap ok");

        assert_eq!(std::fs::read(&current).unwrap(), b"NEW VERSION");
        assert!(
            !live.path().join(".lucerna-update.AppImage.part").exists(),
            "staging file must be renamed away, not left behind",
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_appimage_over_marks_result_executable() {
        use std::os::unix::fs::PermissionsExt;
        let live = tempfile::tempdir().unwrap();
        let current = live.path().join("Lucerna.AppImage");
        std::fs::write(&current, b"old").unwrap();
        std::fs::set_permissions(&current, std::fs::Permissions::from_mode(0o644)).unwrap();
        let new_file = live.path().join("new.AppImage");
        std::fs::write(&new_file, b"new").unwrap();

        install_appimage_over(&current, &new_file).unwrap();

        let mode = std::fs::metadata(&current).unwrap().permissions().mode();
        assert_ne!(
            mode & 0o111,
            0,
            "replaced AppImage must be executable: {mode:o}"
        );
    }
}

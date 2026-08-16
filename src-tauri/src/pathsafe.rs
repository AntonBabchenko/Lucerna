//! Shared path-safety gate: validate that a user-supplied string is a safe
//! single path segment before it is joined onto any directory. Returns a
//! reason string on rejection; callers map it into their own typed error
//! (`worlds` → WorldPathInvalid, `screenshots` → ScreenshotPathInvalid).
//!
//! [`validate_export_dest`] is the other half: not a segment joined onto a
//! directory we own, but a whole destination path the *user* chose in an OS
//! save dialog, which an export command is about to create or overwrite.

use std::path::Path;

/// True iff `name` is a Windows reserved device name (case-insensitive):
/// `CON`/`PRN`/`AUX`/`NUL` and `COM1`..`COM9` / `LPT1`..`LPT9` (`COM0`/`LPT0`
/// are NOT reserved). Shared by [`validate_segment`] and `naming::is_reserved`
/// so the list can't drift between the two gates.
pub fn is_reserved_windows_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "con" | "prn" | "aux" | "nul" => true,
        _ => {
            (lower.starts_with("com") || lower.starts_with("lpt"))
                && lower.len() == 4
                && matches!(lower.as_bytes()[3], b'1'..=b'9')
        }
    }
}

/// `Ok(())` if `name` is a safe single path segment, otherwise `Err(reason)`.
///
/// Rejections: empty; contains `/`, `\`, or `:`; contains `..`; starts with
/// `.`; longer than 255 bytes; case-insensitive Windows reserved name.
pub fn validate_segment(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("empty name");
    }
    if name.contains('/') || name.contains('\\') || name.contains(':') {
        return Err("contains path separator or colon");
    }
    if name.contains("..") {
        return Err("contains '..'");
    }
    if name.starts_with('.') {
        return Err("starts with '.'");
    }
    if name.len() > 255 {
        return Err("longer than 255 bytes");
    }
    if is_reserved_windows_name(name) {
        return Err("Windows reserved name");
    }
    Ok(())
}

/// `true` iff `name` is a safe **single-segment** filename to join under a
/// base directory (a mod/plugin jar name, a platform-supplied asset name).
///
/// This guard screens a name (from a directory listing, a platform API, or
/// user input) before it is joined onto a directory, so it must reject every
/// escape vector on *every* host OS — not just the one we happen to run on.
/// `\` is a path separator and `C:` a drive prefix on Windows, but both are
/// legal filename characters on Unix, so `std::path::Path` parsing alone
/// would let `a\b.jar` / `C:evil.jar` slip through on a Unix build. Screen
/// those explicitly, then require exactly one `Component::Normal` (which
/// catches `/`, `..`, `.`, absolute paths, and empty).
///
/// Unlike [`validate_segment`] this allows leading dots, long names, and
/// Windows reserved stems — jar filenames come from external ecosystems we
/// don't control; this gate only guarantees the join can't escape the
/// directory.
pub fn is_safe_filename(name: &str) -> bool {
    if name.contains('\\') || name.contains(':') {
        return false;
    }
    let mut comps = std::path::Path::new(name).components();
    matches!(comps.next(), Some(std::path::Component::Normal(_))) && comps.next().is_none()
}

/// `Ok(())` if `dest` is a destination an export/save command may write to.
///
/// The export commands (`export_modpack`, `server_export_zip`, `l10n_export`,
/// `save_screenshot_copy`, `save_annotated_screenshot`) take a path the user
/// picked in an OS save dialog and hand it straight to `File::create` /
/// `fs::copy` / `save_with_format`, each of which truncates whatever is
/// already there. Two shapes are never a legitimate save target:
///
/// - A **relative** path. It resolves against the launcher process's current
///   directory — which the user never sees and which differs between a
///   shortcut launch, a shell launch and a dev run. Every real save dialog
///   returns an absolute path, so a relative one means the string did not
///   come from one.
/// - A path **inside the launcher's own program directory**. An export
///   written next to `lucerna.exe` can truncate the binary, a sidecar DLL or
///   a WebView2 runtime file, turning "I exported my modpack" into a broken
///   install.
///
/// `allowed_inside` carves out one directory that stays writable even when it
/// sits inside the program directory. A **portable** install puts the data
/// root at `<exe dir>/LucernaData`, so an instance's `screenshots/` folder —
/// the path the launcher itself proposes as the default for the two
/// screenshot save commands — is legitimately inside the program directory.
/// Without the exemption the default save action would be refused on every
/// portable install. Callers pass the exact directory they own, never a
/// broad ancestor.
///
/// Containment is decided by [`crate::data_root::migrate::is_same_or_nested`],
/// so it is robust against case differences, `\\?\` verbatim prefixes, 8.3
/// short names, and a destination file that does not exist yet.
pub fn validate_export_dest(
    dest: &Path,
    allowed_inside: Option<&Path>,
) -> crate::error::Result<()> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    validate_export_dest_in(dest, allowed_inside, exe_dir.as_deref())
}

/// [`validate_export_dest`] with the program directory injected, so the rules
/// can be tested against a temp directory instead of wherever the test binary
/// happens to live.
fn validate_export_dest_in(
    dest: &Path,
    allowed_inside: Option<&Path>,
    exe_dir: Option<&Path>,
) -> crate::error::Result<()> {
    let deny = |reason: &str| crate::error::Error::io(dest.display().to_string(), reason);

    if dest.as_os_str().is_empty() {
        return Err(deny("empty destination path"));
    }
    // A relative path resolves against the launcher's working directory, not
    // the folder the user picked in the dialog.
    if !dest.is_absolute() {
        return Err(deny("destination must be an absolute path"));
    }
    // Fallback direction: if the process cannot locate its own executable we
    // cannot tell whether `dest` is inside the program directory, so we refuse
    // rather than assume it is not. `current_exe()` failing means /proc is
    // unavailable or the binary was unlinked — not a state in which silently
    // permitting a truncating write is the safe reading. The message says the
    // check could not be performed, not that the path is bad.
    let Some(exe_dir) = exe_dir else {
        return Err(deny("cannot locate the program directory to check against"));
    };
    if !crate::data_root::migrate::is_same_or_nested(exe_dir, dest) {
        return Ok(());
    }
    match allowed_inside {
        Some(ok) if crate::data_root::migrate::is_same_or_nested(ok, dest) => Ok(()),
        _ => Err(deny(
            "refusing to write inside the Lucerna program directory",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_names() {
        assert!(validate_segment("2026-07-07_21.14.30.png").is_ok());
        assert!(validate_segment("My Survival World").is_ok());
        assert!(validate_segment("мир42").is_ok());
    }

    #[test]
    fn rejects_traversal_and_separators() {
        assert!(validate_segment("").is_err());
        assert!(validate_segment("foo/bar").is_err());
        assert!(validate_segment("foo\\bar").is_err());
        assert!(validate_segment("C:foo").is_err());
        assert!(validate_segment("..").is_err());
        assert!(validate_segment("../escape").is_err());
        assert!(validate_segment(".hidden").is_err());
        assert!(validate_segment(&"x".repeat(256)).is_err());
    }

    #[test]
    fn rejects_reserved_windows_names_case_insensitive() {
        for name in &["CON", "con", "Aux", "nul", "COM1", "lpt9"] {
            assert!(
                validate_segment(name).is_err(),
                "expected reject for {name}"
            );
        }
        // com0/lpt0 are NOT reserved.
        assert!(validate_segment("com0").is_ok());
        assert!(validate_segment("lpt0").is_ok());
    }

    #[test]
    fn safe_filename_accepts_plain_names_and_dotfiles() {
        for n in ["sodium-fabric-0.5.3.jar", "a.jar", ".hidden.jar", "CON.jar"] {
            assert!(is_safe_filename(n), "{n} should be a safe filename");
        }
    }

    #[test]
    fn safe_filename_rejects_escape_vectors_on_every_os() {
        for n in [
            "",
            ".",
            "..",
            "../escape.jar",
            "sub/dir.jar",
            "sub\\dir.jar", // Windows separator — must fail on Unix builds too
            "C:evil.jar",   // drive-relative — must fail on Unix builds too
            "C:/x.jar",
            "/abs.jar",
        ] {
            assert!(!is_safe_filename(n), "{n} should be rejected");
        }
    }

    /// A temp dir standing in for an installation: `<td>/app` is the program
    /// directory, `<td>/app/LucernaData/instances/i/.minecraft/screenshots` is
    /// the portable data root's screenshots folder, `<td>/elsewhere` is the
    /// user's own folder. Everything is created so `is_same_or_nested`
    /// canonicalizes real paths rather than best-effort ones.
    fn install_fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let td = tempfile::tempdir().unwrap();
        let exe_dir = td.path().join("app");
        let shots = exe_dir.join("LucernaData/instances/i/.minecraft/screenshots");
        std::fs::create_dir_all(&shots).unwrap();
        std::fs::create_dir_all(td.path().join("elsewhere")).unwrap();
        (td, exe_dir, shots)
    }

    #[test]
    fn export_dest_rejects_empty_and_relative_paths() {
        let (_td, exe_dir, _shots) = install_fixture();
        for bad in ["", "export.mrpack", "./export.mrpack", "sub/export.mrpack"] {
            let r = validate_export_dest_in(Path::new(bad), None, Some(&exe_dir));
            assert!(r.is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn export_dest_accepts_a_folder_outside_the_program_directory() {
        let (td, exe_dir, _shots) = install_fixture();
        let dest = td.path().join("elsewhere/MyPack.mrpack");
        validate_export_dest_in(&dest, None, Some(&exe_dir)).unwrap();
    }

    #[test]
    fn export_dest_refuses_writing_into_the_program_directory() {
        let (_td, exe_dir, _shots) = install_fixture();
        // Directly beside the executable, and nested under it.
        for dest in [exe_dir.join("lucerna.exe"), exe_dir.join("sub/pack.mrpack")] {
            let r = validate_export_dest_in(&dest, None, Some(&exe_dir));
            assert!(r.is_err(), "{} should be refused", dest.display());
        }
    }

    /// The load-bearing exemption: on a portable install the launcher's OWN
    /// default screenshot destination lives inside the program directory. If
    /// the guard refused it, the Save button's default path would break for
    /// every portable user — a shipped regression, not a hardening.
    #[test]
    fn export_dest_allows_the_launchers_own_screenshots_dir_inside_the_program_dir() {
        let (_td, exe_dir, shots) = install_fixture();
        let dest = shots.join("2026-08-16_12.00.00-annotated.png");
        validate_export_dest_in(&dest, Some(&shots), Some(&exe_dir)).unwrap();
    }

    #[test]
    fn export_dest_exemption_does_not_widen_to_the_rest_of_the_program_dir() {
        let (_td, exe_dir, shots) = install_fixture();
        // A sibling of the exempt folder is still inside the program directory.
        let dest = exe_dir.join("LucernaData/instances/i/.minecraft/mods/evil.png");
        let r = validate_export_dest_in(&dest, Some(&shots), Some(&exe_dir));
        assert!(
            r.is_err(),
            "the exemption must cover only the passed folder"
        );
    }

    #[test]
    fn export_dest_does_not_treat_a_name_prefix_as_containment() {
        let (td, exe_dir, _shots) = install_fixture();
        // `<td>/appData` merely starts with the same characters as `<td>/app`.
        let sibling = td.path().join("appData");
        std::fs::create_dir_all(&sibling).unwrap();
        let dest = sibling.join("pack.mrpack");
        validate_export_dest_in(&dest, None, Some(&exe_dir)).unwrap();
    }

    /// Direction check (see the Fallback discipline section of CLAUDE.md): a
    /// check that could not be performed resolves to the restrictive answer.
    #[test]
    fn export_dest_refuses_when_the_program_directory_is_unknown() {
        let (td, _exe_dir, _shots) = install_fixture();
        let dest = td.path().join("elsewhere/MyPack.mrpack");
        let r = validate_export_dest_in(&dest, None, None);
        assert!(
            r.is_err(),
            "unknown program directory must refuse, not allow"
        );
    }
}

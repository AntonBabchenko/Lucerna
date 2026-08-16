//! Shared path-safety gate: validate that a user-supplied string is a safe
//! single path segment before it is joined onto any directory. Returns a
//! reason string on rejection; callers map it into their own typed error
//! (`worlds` → WorldPathInvalid, `screenshots` → ScreenshotPathInvalid).
//!
//! [`validate_export_dest`] is the other half: not a segment joined onto a
//! directory we own, but a whole destination path an export command is about
//! to create or overwrite. It is *supposed* to be what the user chose in an
//! OS save dialog — but the dialog lives in the frontend, so what the command
//! actually receives is a string from the webview, and the guard is written
//! for the case where that string was not the user's choice at all.

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
/// already there. The dialog itself lives in the frontend, so what reaches
/// this function is a string the *webview* supplied — the guard has to hold
/// even when the webview is not telling the truth about where the user
/// pointed. Four shapes are never a legitimate save target:
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
/// - A path **inside the launcher's own state** — the effective data root, or
///   the OS-default app-data dir. On a stock install those are one and the
///   same directory; once the user relocates the root they are not, and the
///   default one *still* holds `data-location.json`, so both have to be
///   checked or the rule has a hole exactly where relocation put it. Nothing
///   under either is an export target: `account.json`, every instance's
///   `instance.json`, the shared mod store. `data-location.json` is the
///   sharpest case — [`crate::data_root::redirect::read`] treats an
///   unparseable file as "no redirect", so truncating it raises no error at
///   all; it silently strands a relocated data root and sends the launcher
///   back to the default one with the user's instances nowhere in sight.
/// - A filename whose **extension is not one this command actually writes**.
///   Everywhere outside the three roots above is fair game for a file the
///   user asked for — but "a file the user asked for" has a name the command
///   knows in advance. Without this rule an export can drop a `.bat` into the
///   Startup folder, a `.desktop` into `autostart/`, or overwrite a dotfile,
///   with content the caller influences (`save_annotated_screenshot`
///   composites caller-supplied pixels). `allowed_extensions` is matched
///   case-insensitively, and an extensionless destination is refused too —
///   `~/.bashrc` has no extension.
///
///   The final component is separately refused if it ends in a dot or a
///   space. Windows strips both when opening a file, so `evil.bat.` reaches
///   the disk as `evil.bat`; `Path::extension` sees `""` there and would
///   already refuse it, but naming the trick explicitly is what keeps the
///   rule from silently depending on that coincidence.
///
/// `allowed_inside` carves out one directory that stays writable even when it
/// sits inside a protected root, and it applies to *every* containment rule
/// above because the folder it exists for sits inside a different one on each
/// install shape. An instance's `screenshots/` folder — the path the launcher
/// itself proposes as the default for the two screenshot save commands — is
/// under the data root on a stock install, and under the program directory as
/// well on a **portable** one (where the data root is `<exe dir>/LucernaData`).
/// Without the exemption the default save action would be refused for every
/// user. Callers pass the exact directory they own, never a broad ancestor.
///
/// Containment is decided by [`crate::data_root::migrate::is_same_or_nested`],
/// so it is robust against case differences, `\\?\` verbatim prefixes, 8.3
/// short names, and a destination file that does not exist yet.
pub fn validate_export_dest(
    app: &tauri::AppHandle,
    dest: &Path,
    allowed_inside: Option<&Path>,
    allowed_extensions: &[&str],
) -> crate::error::Result<()> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let data_root = crate::paths::app_dir(app).ok();
    let default_data_dir = crate::paths::default_app_data_dir(app).ok();
    validate_export_dest_in(
        dest,
        allowed_inside,
        allowed_extensions,
        exe_dir.as_deref(),
        data_root.as_deref(),
        default_data_dir.as_deref(),
    )
}

/// [`validate_export_dest`] with the protected roots injected, so the rules
/// can be tested against a temp directory instead of wherever the test binary
/// happens to live and whatever the host's real app-data dir is.
fn validate_export_dest_in(
    dest: &Path,
    allowed_inside: Option<&Path>,
    allowed_extensions: &[&str],
    exe_dir: Option<&Path>,
    data_root: Option<&Path>,
    default_data_dir: Option<&Path>,
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

    // A name we cannot read as UTF-8 is a name we cannot reason about; a path
    // ending in `..` or a root has no final component at all.
    let Some(file_name) = dest.file_name().and_then(|n| n.to_str()) else {
        return Err(deny("destination has no readable file name"));
    };
    // Windows strips trailing dots and spaces when it opens a file, so
    // `evil.bat.` and `x.bat . ` both land on disk as something other than
    // what the name says. Refuse the shape outright rather than reasoning
    // about what the OS will trim.
    if file_name.ends_with('.') || file_name.ends_with(' ') {
        return Err(deny(
            "destination file name must not end with a dot or a space",
        ));
    }
    let ext = dest
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    if !allowed_extensions
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(ext))
    {
        return Err(deny(&format!(
            "destination must end in .{}",
            allowed_extensions.join(" or .")
        )));
    }

    let exempt =
        allowed_inside.is_some_and(|ok| crate::data_root::migrate::is_same_or_nested(ok, dest));
    for (what, root) in [
        ("program", exe_dir),
        ("data", data_root),
        ("app-data", default_data_dir),
    ] {
        // Fallback direction: if a root could not be resolved we cannot tell
        // whether `dest` is inside it, so we refuse rather than assume it is
        // not. `current_exe()` failing means /proc is unavailable or the
        // binary was unlinked; `app_dir` failing means the platform would not
        // name an app-data dir — neither is a state in which silently
        // permitting a truncating write is the safe reading. The message says
        // the check could not be performed, not that the path is bad.
        let Some(root) = root else {
            return Err(deny(&format!(
                "cannot locate the Lucerna {what} directory to check against"
            )));
        };
        if !exempt && crate::data_root::migrate::is_same_or_nested(root, dest) {
            return Err(deny(&format!(
                "refusing to write inside the Lucerna {what} directory"
            )));
        }
    }
    Ok(())
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

    /// A temp dir standing in for an installation. `<td>/app` is the program
    /// directory, `<td>/appdata/com.lucerna.app` is the OS-default app-data
    /// dir, `<td>/user` is the user's own folder, and `shots` is an
    /// instance's screenshots folder under whichever tree the data root is
    /// in. Everything is created so `is_same_or_nested` canonicalizes real
    /// paths rather than best-effort ones.
    struct Install {
        _td: tempfile::TempDir,
        exe_dir: std::path::PathBuf,
        data_root: std::path::PathBuf,
        default_data_dir: std::path::PathBuf,
        shots: std::path::PathBuf,
        user_dir: std::path::PathBuf,
    }

    impl Install {
        /// `portable = false` is the stock shape: the data root IS the
        /// OS-default app-data dir, a different tree from the program
        /// directory — the shape the data-root rule exists for.
        /// `portable = true` puts the data root at `<exe dir>/LucernaData`,
        /// the shape the screenshots exemption exists for.
        fn new(portable: bool) -> Self {
            let td = tempfile::tempdir().unwrap();
            let exe_dir = td.path().join("app");
            let default_data_dir = td.path().join("appdata/com.lucerna.app");
            let data_root = if portable {
                exe_dir.join("LucernaData")
            } else {
                default_data_dir.clone()
            };
            let shots = data_root.join("instances/i/.minecraft/screenshots");
            let user_dir = td.path().join("user");
            for d in [&exe_dir, &default_data_dir, &data_root, &shots, &user_dir] {
                std::fs::create_dir_all(d).unwrap();
            }
            Self {
                _td: td,
                exe_dir,
                data_root,
                default_data_dir,
                shots,
                user_dir,
            }
        }

        /// Run the validator with this install's three protected roots injected.
        fn check(
            &self,
            dest: &Path,
            allowed_inside: Option<&Path>,
            allowed_extensions: &[&str],
        ) -> crate::error::Result<()> {
            validate_export_dest_in(
                dest,
                allowed_inside,
                allowed_extensions,
                Some(&self.exe_dir),
                Some(&self.data_root),
                Some(&self.default_data_dir),
            )
        }
    }

    /// The extensions the *containment* tests' destinations happen to use.
    /// Those tests are about **where** a write may land, not **what** it may
    /// be called; the extension rule has its own tests further down.
    const ANY_EXT: &[&str] = &["mrpack", "png", "json", "jar", "exe"];

    #[test]
    fn export_dest_rejects_empty_and_relative_paths() {
        let inst = Install::new(false);
        for bad in ["", "export.mrpack", "./export.mrpack", "sub/export.mrpack"] {
            assert!(
                inst.check(Path::new(bad), None, ANY_EXT).is_err(),
                "{bad:?} should be refused"
            );
        }
    }

    #[test]
    fn export_dest_accepts_a_folder_the_launcher_does_not_own() {
        let inst = Install::new(false);
        let dest = inst.user_dir.join("MyPack.mrpack");
        inst.check(&dest, None, ANY_EXT).unwrap();
    }

    #[test]
    fn export_dest_refuses_writing_into_the_program_directory() {
        let inst = Install::new(false);
        // Directly beside the executable, and nested under it.
        for dest in [
            inst.exe_dir.join("lucerna.exe"),
            inst.exe_dir.join("sub/pack.mrpack"),
        ] {
            assert!(
                inst.check(&dest, None, ANY_EXT).is_err(),
                "{} should be refused",
                dest.display()
            );
        }
    }

    /// The rule the program-directory check does NOT cover: on a stock
    /// install the data root is a different tree from the exe dir, so every
    /// one of these was reachable while only the program directory was
    /// screened. `data-location.json` is the one that fails silently —
    /// `redirect::read` reads an unparseable file as "no redirect", so a
    /// truncated one strands a relocated data root with no error anywhere.
    #[test]
    fn export_dest_refuses_writing_into_the_data_root() {
        let inst = Install::new(false);
        for rel in [
            "data-location.json",
            "account.json",
            "instances/i/instance.json",
            "instances/i/.minecraft/mods/sodium.jar",
        ] {
            let dest = inst.data_root.join(rel);
            assert!(
                inst.check(&dest, None, ANY_EXT).is_err(),
                "{rel} inside the data root should be refused"
            );
        }
    }

    /// Once the root is relocated the two are different trees, and the
    /// OS-default one still holds `data-location.json`. Checking only the
    /// effective root would leave the redirect file writable on exactly the
    /// installs that depend on it.
    #[test]
    fn export_dest_refuses_the_default_app_data_dir_even_when_the_root_moved() {
        let inst = Install::new(true);
        // Precondition: this really is a tree the data-root rule does not cover.
        assert!(!crate::data_root::migrate::is_same_or_nested(
            &inst.data_root,
            &inst.default_data_dir
        ));
        let dest = inst.default_data_dir.join("data-location.json");
        assert!(inst.check(&dest, None, ANY_EXT).is_err());
    }

    /// The load-bearing exemption. On a stock install the launcher's OWN
    /// default screenshot destination lives inside the data root; on a
    /// portable one it is inside the program directory as well. If the guard
    /// refused it, the Save button's default path would break for every user
    /// — a shipped regression, not a hardening.
    #[test]
    fn export_dest_allows_the_launchers_own_screenshots_dir_on_both_install_shapes() {
        for portable in [false, true] {
            let inst = Install::new(portable);
            let dest = inst.shots.join("2026-08-16_12.00.00-annotated.png");
            inst.check(&dest, Some(&inst.shots), ANY_EXT)
                .unwrap_or_else(|e| panic!("portable={portable}: {e:?}"));
        }
    }

    #[test]
    fn export_dest_exemption_does_not_widen_to_the_rest_of_the_protected_roots() {
        for portable in [false, true] {
            let inst = Install::new(portable);
            // A sibling of the exempt folder is still inside the data root.
            let sibling = inst.data_root.join("instances/i/.minecraft/mods/evil.png");
            assert!(
                inst.check(&sibling, Some(&inst.shots), ANY_EXT).is_err(),
                "portable={portable}: the exemption must cover only the passed folder"
            );
            // And so is the redirect file, exemption or not.
            let redirect = inst.default_data_dir.join("data-location.json");
            assert!(
                inst.check(&redirect, Some(&inst.shots), ANY_EXT).is_err(),
                "portable={portable}: the exemption must not reach the redirect file"
            );
        }
    }

    #[test]
    fn export_dest_does_not_treat_a_name_prefix_as_containment() {
        let inst = Install::new(false);
        // `<td>/app-notes` merely starts with the same characters as `<td>/app`.
        let sibling = inst.exe_dir.with_file_name("app-notes");
        std::fs::create_dir_all(&sibling).unwrap();
        let dest = sibling.join("pack.mrpack");
        inst.check(&dest, None, ANY_EXT).unwrap();
    }

    /// Outside the protected roots the destination is the user's own folder,
    /// so containment says nothing — the only thing standing between an
    /// export and `%APPDATA%\..\Start Menu\...\Startup\run.bat` is the name.
    #[test]
    fn export_dest_requires_an_extension_the_command_actually_writes() {
        let inst = Install::new(false);
        for bad in [
            "run.bat",
            "run.cmd",
            "hook.ps1",
            "lucerna.desktop",
            "libthing.so",
            "notes",           // no extension at all
            ".bashrc",         // a dotfile has no extension either
            "pack.mrpack.zip", // the LAST extension is the one that counts
        ] {
            let dest = inst.user_dir.join(bad);
            assert!(
                inst.check(&dest, None, &["mrpack"]).is_err(),
                "{bad:?} should be refused"
            );
        }
    }

    #[test]
    fn export_dest_accepts_any_extension_on_the_list_case_insensitively() {
        let inst = Install::new(false);
        for ok in ["pack.mrpack", "pack.zip", "pack.MRPACK", "pack.Zip"] {
            let dest = inst.user_dir.join(ok);
            inst.check(&dest, None, &["mrpack", "zip"])
                .unwrap_or_else(|e| panic!("{ok} should be accepted: {e:?}"));
        }
    }

    /// Windows trims trailing dots and spaces when it opens a file, so a name
    /// that reads as `.png` can still land on disk as `.bat`.
    #[test]
    fn export_dest_rejects_windows_trailing_dot_and_space_names() {
        let inst = Install::new(false);
        for bad in ["shot.png.", "shot.png ", "evil.bat.", "evil.bat . "] {
            let dest = inst.user_dir.join(bad);
            assert!(
                inst.check(&dest, None, &["png"]).is_err(),
                "{bad:?} should be refused"
            );
        }
    }

    /// The exemption is about **where**, never about **what**. The
    /// screenshots folder stays writable, but not under any name the caller
    /// likes — `save_annotated_screenshot` composites caller-supplied pixels.
    #[test]
    fn export_dest_exemption_does_not_bypass_the_extension_rule() {
        let inst = Install::new(true);
        let dest = inst.shots.join("evil.bat");
        assert!(inst.check(&dest, Some(&inst.shots), &["png"]).is_err());
    }

    /// Direction check (see the Fallback discipline section of CLAUDE.md): a
    /// check that could not be performed resolves to the restrictive answer —
    /// for every root, not just the first one.
    #[test]
    fn export_dest_refuses_when_a_protected_root_is_unknown() {
        let inst = Install::new(false);
        let dest = inst.user_dir.join("MyPack.mrpack");
        // Sanity: with all three known this destination is allowed.
        inst.check(&dest, None, ANY_EXT).unwrap();
        for (label, roots) in [
            (
                "program",
                (None, Some(&inst.data_root), Some(&inst.default_data_dir)),
            ),
            (
                "data",
                (Some(&inst.exe_dir), None, Some(&inst.default_data_dir)),
            ),
            (
                "app-data",
                (Some(&inst.exe_dir), Some(&inst.data_root), None),
            ),
        ] {
            let (exe, data, app_data) = roots;
            let r = validate_export_dest_in(
                &dest,
                None,
                ANY_EXT,
                exe.map(|p| p.as_path()),
                data.map(|p| p.as_path()),
                app_data.map(|p| p.as_path()),
            );
            assert!(
                r.is_err(),
                "an unknown {label} directory must refuse, not allow"
            );
        }
    }
}

//! Data-root location: bootstrap redirect file + resolution + migration.
pub mod migrate;
pub mod redirect;
pub mod validate;

use redirect::Redirect;
use std::path::{Path, PathBuf};
use tauri::Manager;

/// Resolved effective data root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The effective root every path derives from.
    pub root: PathBuf,
    /// The user's configured custom root, if any (independent of availability).
    pub configured: Option<PathBuf>,
    /// True when a configured root exists but is unavailable → running from
    /// `default` temporarily.
    pub fell_back: bool,
    /// True when `root` is a portable dir that does not exist yet — the caller
    /// must create it before use (and fall back to the OS default if creation
    /// fails).
    pub must_create: bool,
}

/// The `<exe dir>\LucernaData` portable candidate, as observed by the caller.
/// Observations are injected (not probed here) so resolution stays pure. The
/// caller passes `None` in dev builds and whenever a redirect exists, so
/// `pnpm tauri dev` never adopts `target/debug/LucernaData` and the install
/// dir is never write-probed when an explicit choice already wins.
pub struct PortableCandidate {
    pub path: PathBuf,
    /// The directory already exists → adopt it (this is what re-attaches data
    /// after an uninstall-keep-data → reinstall-into-same-folder cycle, with
    /// no pointer file involved).
    pub exists: bool,
    /// The exe's directory is writable → a fresh start may create the dir.
    pub creatable: bool,
}

/// Pure resolution. `available` is injected so this is testable without a real
/// filesystem (production passes a probe that checks exists + writable).
///
/// Precedence:
/// 1. redirect (explicit user choice; unavailable → `fell_back` onto `default`,
///    portable candidates deliberately NOT considered for the temporary root —
///    data-writing commands are blocked in that state anyway);
/// 2. existing `<exe dir>\LucernaData` (portable adopt — deliberately beats a
///    data-carrying default so reinstall-into-same-folder always re-attaches;
///    a planted exe-side dir shadowing legacy `%APPDATA%` data is the accepted
///    trade-off, documented in the spec);
/// 3. `default` when it already holds data (existing installs unchanged);
/// 4. creatable portable dir (fresh installs keep everything next to the exe);
/// 5. `default`.
pub fn resolve_root(
    default: PathBuf,
    default_has_data: bool,
    portable: Option<PortableCandidate>,
    redirect: Option<Redirect>,
    available: impl Fn(&Path) -> bool,
) -> Resolved {
    match redirect {
        Some(Redirect { path }) if available(&path) => {
            return Resolved {
                root: path.clone(),
                configured: Some(path),
                fell_back: false,
                must_create: false,
            }
        }
        Some(Redirect { path }) => {
            return Resolved {
                root: default,
                configured: Some(path),
                fell_back: true,
                must_create: false,
            }
        }
        None => {}
    }
    if let Some(candidate) = portable {
        if candidate.exists {
            return Resolved {
                root: candidate.path,
                configured: None,
                fell_back: false,
                must_create: false,
            };
        }
        if !default_has_data && candidate.creatable {
            return Resolved {
                root: candidate.path,
                configured: None,
                fell_back: false,
                must_create: true,
            };
        }
    }
    Resolved {
        root: default,
        configured: None,
        fell_back: false,
        must_create: false,
    }
}

/// Tauri managed state holding the resolution result.
pub struct DataRoot(pub Resolved);

/// Integrity chokepoint: reject a data-creating or launching command when the
/// configured data root is unavailable and we are running from the default
/// fallback. The UI already gates these actions, but a direct IPC call could
/// bypass that and write into the wrong root — so every create/launch command
/// calls this at its top as a defence-in-depth backstop.
pub fn reject_if_fallen_back(app: &tauri::AppHandle) -> crate::error::Result<()> {
    if app.state::<DataRoot>().0.fell_back {
        return Err(crate::error::Error::DataLocationUnavailable);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def() -> PathBuf {
        PathBuf::from("/default")
    }

    fn portable(exists: bool, creatable: bool) -> Option<PortableCandidate> {
        Some(PortableCandidate {
            path: PathBuf::from("/install/LucernaData"),
            exists,
            creatable,
        })
    }

    #[test]
    fn no_redirect_no_portable_uses_default() {
        let r = resolve_root(def(), false, None, None, |_| true);
        assert_eq!(r.root, def());
        assert!(!r.fell_back && r.configured.is_none() && !r.must_create);
    }

    #[test]
    fn available_custom_is_used() {
        let red = Some(Redirect {
            path: PathBuf::from("/custom"),
        });
        let r = resolve_root(def(), false, None, red, |_| true);
        assert_eq!(r.root, PathBuf::from("/custom"));
        assert_eq!(r.configured, Some(PathBuf::from("/custom")));
        assert!(!r.fell_back && !r.must_create);
    }

    #[test]
    fn unavailable_custom_falls_back_but_keeps_configured() {
        let red = Some(Redirect {
            path: PathBuf::from("/custom"),
        });
        let r = resolve_root(def(), false, None, red, |_| false);
        assert_eq!(r.root, def());
        assert_eq!(r.configured, Some(PathBuf::from("/custom")));
        assert!(r.fell_back && !r.must_create);
    }

    #[test]
    fn existing_exe_side_dir_is_adopted() {
        let r = resolve_root(def(), false, portable(true, true), None, |_| true);
        assert_eq!(r.root, PathBuf::from("/install/LucernaData"));
        assert!(r.configured.is_none() && !r.fell_back && !r.must_create);
    }

    #[test]
    fn redirect_beats_existing_exe_side_dir() {
        // Callers skip building the candidate when a redirect exists, but the
        // precedence must hold even if one is passed.
        let red = Some(Redirect {
            path: PathBuf::from("/custom"),
        });
        let r = resolve_root(def(), false, portable(true, true), red, |_| true);
        assert_eq!(r.root, PathBuf::from("/custom"));
    }

    #[test]
    fn existing_exe_side_dir_beats_data_carrying_default() {
        // Documented trade-off: deterministic re-adoption after reinstall
        // outweighs the planted-dir shadowing risk.
        let r = resolve_root(def(), true, portable(true, true), None, |_| true);
        assert_eq!(r.root, PathBuf::from("/install/LucernaData"));
    }

    #[test]
    fn fresh_start_creates_portable_dir() {
        let r = resolve_root(def(), false, portable(false, true), None, |_| true);
        assert_eq!(r.root, PathBuf::from("/install/LucernaData"));
        assert!(r.must_create);
    }

    #[test]
    fn data_carrying_default_blocks_portable_creation() {
        // Existing installs must not sprout a second, empty root next to the
        // exe — their %APPDATA% data keeps winning.
        let r = resolve_root(def(), true, portable(false, true), None, |_| true);
        assert_eq!(r.root, def());
        assert!(!r.must_create);
    }

    #[test]
    fn unwritable_install_dir_falls_back_to_default() {
        let r = resolve_root(def(), false, portable(false, false), None, |_| true);
        assert_eq!(r.root, def());
        assert!(!r.must_create);
    }

    #[test]
    fn fallback_root_stays_default_even_with_exe_side_dir() {
        // An unavailable redirect means "temporary safe root until the drive
        // returns" — data-writing commands are rejected in that state, so the
        // temporary root must stay the stable OS default, not silently switch
        // to a portable dir the user never chose.
        let red = Some(Redirect {
            path: PathBuf::from("/usb"),
        });
        let r = resolve_root(def(), false, portable(true, true), red, |_| false);
        assert_eq!(r.root, def());
        assert!(r.fell_back);
    }
}

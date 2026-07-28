//! Data-root location: bootstrap redirect file + resolution + migration.
pub mod migrate;
pub mod plan;
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

/// How the `<exe dir>\LucernaData` portable candidate looks on disk, as
/// observed by the caller (injected, not probed here, so resolution stays
/// pure). The caller passes the whole candidate as `None` in dev builds and
/// whenever a redirect exists, so `pnpm tauri dev` never adopts
/// `target/debug/LucernaData` and the install dir is never write-probed when
/// an explicit choice already wins.
pub enum PortableState {
    /// No such directory. `creatable` = the exe's directory is writable, so a
    /// fresh start may create it.
    Absent { creatable: bool },
    /// Exists and looks like a Lucerna data root (`looks_like_data_root`) →
    /// adopt it. This is what re-attaches data after an
    /// uninstall-keep-data → reinstall-into-same-folder cycle, with no
    /// pointer file involved.
    Root,
    /// Exists and is empty → usable as a fresh root as-is.
    EmptyDir,
    /// Exists with unrecognized content. NEVER adopted: writing Lucerna data
    /// into a foreign folder would entangle it with files the launcher may
    /// later migrate or the uninstaller may delete.
    Foreign,
}

pub struct PortableCandidate {
    pub path: PathBuf,
    pub state: PortableState,
}

/// Shape test for "this directory is (or was) a Lucerna data root": an
/// `instances/` directory plus an `app.json` that parses as a JSON object.
/// `versions/` is deliberately not required — a fresh root lacks it until the
/// first version install. Keep in sync with the Storage panel's adopt-flow
/// detector (`data_root::plan::is_data_root` on its branch); the two are
/// meant to merge into one function once both land.
pub fn looks_like_data_root(dir: &Path) -> bool {
    if !dir.join("instances").is_dir() {
        return false;
    }
    std::fs::read_to_string(dir.join("app.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .is_some_and(|v| v.is_object())
}

/// Pure resolution. `available` is injected so this is testable without a real
/// filesystem (production passes a probe that checks exists + writable).
///
/// Precedence:
/// 1. redirect (explicit user choice; unavailable → `fell_back` onto `default`,
///    portable candidates deliberately NOT considered for the temporary root —
///    data-writing commands are blocked in that state anyway);
/// 2. `<exe dir>\LucernaData` that is a data root or empty (portable adopt —
///    deliberately beats a data-carrying default so
///    reinstall-into-same-folder always re-attaches; a root-shaped planted
///    dir shadowing legacy `%APPDATA%` data is the accepted trade-off,
///    documented in the spec; a foreign dir is never touched);
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
        match candidate.state {
            // A root-shaped dir always wins — this is the re-adoption path,
            // and real data next to the exe outranks whatever the default
            // holds (documented trade-off).
            PortableState::Root => {
                return Resolved {
                    root: candidate.path,
                    configured: None,
                    fell_back: false,
                    must_create: false,
                };
            }
            // A merely-EMPTY dir carries no data and therefore must not
            // shadow a data-carrying default — it only serves as the fresh
            // root of a fresh install.
            PortableState::EmptyDir if !default_has_data => {
                return Resolved {
                    root: candidate.path,
                    configured: None,
                    fell_back: false,
                    must_create: false,
                };
            }
            PortableState::Absent { creatable } if creatable && !default_has_data => {
                return Resolved {
                    root: candidate.path,
                    configured: None,
                    fell_back: false,
                    must_create: true,
                };
            }
            PortableState::Absent { .. } | PortableState::EmptyDir | PortableState::Foreign => {}
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

    fn portable(state: PortableState) -> Option<PortableCandidate> {
        Some(PortableCandidate {
            path: PathBuf::from("/install/LucernaData"),
            state,
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
    fn root_shaped_exe_side_dir_is_adopted() {
        let r = resolve_root(def(), false, portable(PortableState::Root), None, |_| true);
        assert_eq!(r.root, PathBuf::from("/install/LucernaData"));
        assert!(r.configured.is_none() && !r.fell_back && !r.must_create);
    }

    #[test]
    fn empty_exe_side_dir_is_used_as_fresh_root() {
        let r = resolve_root(
            def(),
            false,
            portable(PortableState::EmptyDir),
            None,
            |_| true,
        );
        assert_eq!(r.root, PathBuf::from("/install/LucernaData"));
        assert!(!r.must_create);
    }

    #[test]
    fn empty_exe_side_dir_does_not_shadow_data_carrying_default() {
        // Only a ROOT-shaped dir may outrank existing default data; an empty
        // coincidentally-named folder holds nothing worth switching to and
        // would make the user's instances "vanish".
        let r = resolve_root(def(), true, portable(PortableState::EmptyDir), None, |_| {
            true
        });
        assert_eq!(r.root, def());
    }

    #[test]
    fn foreign_exe_side_dir_is_never_adopted() {
        // A folder that merely SHARES the LucernaData name but holds
        // unrecognized content must not become a data root: the launcher
        // would entangle its data with foreign files that a later migration
        // or uninstall-cleanup would then destroy.
        let r = resolve_root(def(), false, portable(PortableState::Foreign), None, |_| {
            true
        });
        assert_eq!(r.root, def());
        assert!(!r.must_create);
    }

    #[test]
    fn redirect_beats_root_shaped_exe_side_dir() {
        // Callers skip building the candidate when a redirect exists, but the
        // precedence must hold even if one is passed.
        let red = Some(Redirect {
            path: PathBuf::from("/custom"),
        });
        let r = resolve_root(def(), false, portable(PortableState::Root), red, |_| true);
        assert_eq!(r.root, PathBuf::from("/custom"));
    }

    #[test]
    fn root_shaped_exe_side_dir_beats_data_carrying_default() {
        // Documented trade-off: deterministic re-adoption after reinstall
        // outweighs the planted-dir shadowing risk.
        let r = resolve_root(def(), true, portable(PortableState::Root), None, |_| true);
        assert_eq!(r.root, PathBuf::from("/install/LucernaData"));
    }

    #[test]
    fn fresh_start_creates_portable_dir() {
        let r = resolve_root(
            def(),
            false,
            portable(PortableState::Absent { creatable: true }),
            None,
            |_| true,
        );
        assert_eq!(r.root, PathBuf::from("/install/LucernaData"));
        assert!(r.must_create);
    }

    #[test]
    fn data_carrying_default_blocks_portable_creation() {
        // Existing installs must not sprout a second, empty root next to the
        // exe — their %APPDATA% data keeps winning.
        let r = resolve_root(
            def(),
            true,
            portable(PortableState::Absent { creatable: true }),
            None,
            |_| true,
        );
        assert_eq!(r.root, def());
        assert!(!r.must_create);
    }

    #[test]
    fn unwritable_install_dir_falls_back_to_default() {
        let r = resolve_root(
            def(),
            false,
            portable(PortableState::Absent { creatable: false }),
            None,
            |_| true,
        );
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
        let r = resolve_root(def(), false, portable(PortableState::Root), red, |_| false);
        assert_eq!(r.root, def());
        assert!(r.fell_back);
    }

    #[test]
    fn looks_like_data_root_requires_instances_and_parseable_app_json() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        assert!(!looks_like_data_root(dir), "empty dir is not a root");

        std::fs::create_dir_all(dir.join("instances")).unwrap();
        assert!(
            !looks_like_data_root(dir),
            "instances alone is not enough (foreign dirs can have one)"
        );

        std::fs::write(dir.join("app.json"), "not json").unwrap();
        assert!(!looks_like_data_root(dir), "unparseable app.json rejected");

        std::fs::write(dir.join("app.json"), "[1,2]").unwrap();
        assert!(!looks_like_data_root(dir), "non-object app.json rejected");

        std::fs::write(dir.join("app.json"), r#"{"active_instance":null}"#).unwrap();
        assert!(looks_like_data_root(dir));
    }
}

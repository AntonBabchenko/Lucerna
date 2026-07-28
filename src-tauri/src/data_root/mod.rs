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
}

/// Pure resolution. `available` is injected so this is testable without a real
/// filesystem (production passes a probe that checks exists + writable).
pub fn resolve_root(
    default: PathBuf,
    redirect: Option<Redirect>,
    available: impl Fn(&Path) -> bool,
) -> Resolved {
    match redirect {
        None => Resolved {
            root: default,
            configured: None,
            fell_back: false,
        },
        Some(Redirect { path }) if available(&path) => Resolved {
            root: path.clone(),
            configured: Some(path),
            fell_back: false,
        },
        Some(Redirect { path }) => Resolved {
            root: default,
            configured: Some(path),
            fell_back: true,
        },
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

    #[test]
    fn no_redirect_uses_default() {
        let r = resolve_root(def(), None, |_| true);
        assert_eq!(r.root, def());
        assert!(!r.fell_back && r.configured.is_none());
    }

    #[test]
    fn available_custom_is_used() {
        let red = Some(Redirect {
            path: PathBuf::from("/custom"),
        });
        let r = resolve_root(def(), red, |_| true);
        assert_eq!(r.root, PathBuf::from("/custom"));
        assert_eq!(r.configured, Some(PathBuf::from("/custom")));
        assert!(!r.fell_back);
    }

    #[test]
    fn unavailable_custom_falls_back_but_keeps_configured() {
        let red = Some(Redirect {
            path: PathBuf::from("/custom"),
        });
        let r = resolve_root(def(), red, |_| false);
        assert_eq!(r.root, def());
        assert_eq!(r.configured, Some(PathBuf::from("/custom")));
        assert!(r.fell_back);
    }
}

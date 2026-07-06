//! Data-root location: bootstrap redirect file + resolution + migration.
pub mod migrate;
pub mod redirect;
pub mod validate;

use redirect::Redirect;
use std::path::{Path, PathBuf};

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

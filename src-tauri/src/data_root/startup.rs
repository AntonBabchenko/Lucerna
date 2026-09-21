//! Where will the launcher's data root be on the NEXT start?
//!
//! Extracted unchanged from `lib.rs` setup so that the data-root move can ask
//! the real resolver — not a re-implementation of its precedence rules —
//! whether a committed switch will actually take effect. It matters: with no
//! redirect, a root-shaped `<exe dir>\LucernaData` beats a data-carrying
//! OS-default dir (`resolve_root` rule 2), so removing the redirect alone does
//! not move a portable install anywhere.

use crate::data_root::{
    looks_like_data_root, migrate, redirect, resolve_root, PortableCandidate, PortableState,
    Resolved,
};
use std::path::{Path, PathBuf};

pub struct StartupInputs {
    /// The OS-default app-data dir.
    pub default_root: PathBuf,
    /// `<default_root>/data-location.json`; `None` when it could not be built.
    pub redirect_file: Option<PathBuf>,
    /// The directory holding the running executable, when known.
    pub exe_dir: Option<PathBuf>,
    /// Whether `<exe dir>\LucernaData` may be considered at all. Production
    /// passes [`portable_allowed`]; tests pass what they need.
    pub portable_allowed: bool,
}

/// Portable candidate: WINDOWS release builds only. Dev must never adopt
/// `target/debug/LucernaData`; macOS is excluded deliberately — a `.app`
/// bundle dir is often writable, but the only macOS update path is Finder's
/// drag-replace, which DELETES the old bundle wholesale: data created inside
/// it would be lost on every update. (Linux is naturally safe — AppImage
/// mounts read-only, deb/rpm install to root-owned paths — but stays excluded
/// until portable mode is designed for it.)
pub fn portable_allowed() -> bool {
    cfg!(all(windows, not(debug_assertions)))
}

fn portable_candidate(exe_dir: &Path) -> PortableCandidate {
    let path = exe_dir.join("LucernaData");
    let state = if !path.is_dir() {
        PortableState::Absent {
            // Probe writability only when creation could actually happen.
            creatable: migrate::is_available(exe_dir),
        }
    } else if looks_like_data_root(&path) {
        PortableState::Root
    } else if migrate::target_is_empty(&path) {
        PortableState::EmptyDir
    } else {
        PortableState::Foreign
    };
    PortableCandidate { path, state }
}

/// The root resolution `lib.rs` runs at startup. Probes the filesystem; holds
/// no state. `must_create` handling and the log line stay with the caller.
pub fn resolve_at_startup(inputs: &StartupInputs) -> Resolved {
    let redirect = inputs
        .redirect_file
        .as_deref()
        .and_then(|f| redirect::read(f).ok().flatten());
    // The install dir must not be write-probed when the user's explicit
    // choice wins anyway, so the candidate exists only without a redirect.
    let portable = if !inputs.portable_allowed || redirect.is_some() {
        None
    } else {
        inputs.exe_dir.as_deref().map(portable_candidate)
    };
    let default_has_data = inputs.default_root.join("app.json").is_file()
        || inputs.default_root.join("instances").is_dir();
    resolve_root(
        inputs.default_root.clone(),
        default_has_data,
        portable,
        redirect,
        |p| migrate::is_available(p),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_root::redirect::Redirect;
    use tempfile::{tempdir, TempDir};

    fn make_root(dir: &Path) {
        std::fs::create_dir_all(dir.join("instances")).unwrap();
        std::fs::write(dir.join("app.json"), b"{}").unwrap();
    }

    struct World {
        _d: TempDir,
        default_root: PathBuf,
        exe_dir: PathBuf,
    }

    fn world() -> World {
        let d = tempdir().unwrap();
        let default_root = d.path().join("appdata");
        let exe_dir = d.path().join("install");
        std::fs::create_dir_all(&default_root).unwrap();
        std::fs::create_dir_all(&exe_dir).unwrap();
        World {
            _d: d,
            default_root,
            exe_dir,
        }
    }

    fn inputs(w: &World, portable_allowed: bool) -> StartupInputs {
        StartupInputs {
            default_root: w.default_root.clone(),
            redirect_file: Some(w.default_root.join("data-location.json")),
            exe_dir: Some(w.exe_dir.clone()),
            portable_allowed,
        }
    }

    #[test]
    fn an_available_redirect_wins_over_everything() {
        let w = world();
        make_root(&w.default_root);
        make_root(&w.exe_dir.join("LucernaData"));
        let custom = w._d.path().join("custom");
        make_root(&custom);
        redirect::write(
            &w.default_root.join("data-location.json"),
            &Redirect {
                path: custom.clone(),
            },
        )
        .unwrap();
        let r = resolve_at_startup(&inputs(&w, true));
        assert_eq!(r.root, custom);
        assert!(!r.fell_back);
    }

    #[test]
    fn an_unavailable_redirect_falls_back_onto_the_default() {
        let w = world();
        redirect::write(
            &w.default_root.join("data-location.json"),
            &Redirect {
                path: w._d.path().join("unplugged"),
            },
        )
        .unwrap();
        let r = resolve_at_startup(&inputs(&w, true));
        assert_eq!(r.root, w.default_root);
        assert!(r.fell_back);
    }

    #[test]
    fn without_a_redirect_a_root_shaped_exe_side_folder_beats_a_data_carrying_default() {
        // This is WHY a reset away from the exe-side root must hide its app.json.
        let w = world();
        make_root(&w.default_root);
        make_root(&w.exe_dir.join("LucernaData"));
        let r = resolve_at_startup(&inputs(&w, true));
        assert_eq!(r.root, w.exe_dir.join("LucernaData"));
    }

    #[test]
    fn a_hidden_exe_side_root_no_longer_shadows_the_default() {
        let w = world();
        make_root(&w.default_root);
        let exe_side = w.exe_dir.join("LucernaData");
        make_root(&exe_side);
        std::fs::rename(exe_side.join("app.json"), exe_side.join("app.json.moved")).unwrap();
        let r = resolve_at_startup(&inputs(&w, true));
        assert_eq!(r.root, w.default_root);
        assert!(!r.fell_back);
    }

    #[test]
    fn when_portable_is_not_allowed_the_exe_side_folder_is_never_considered() {
        let w = world();
        make_root(&w.default_root);
        make_root(&w.exe_dir.join("LucernaData"));
        let r = resolve_at_startup(&inputs(&w, false));
        assert_eq!(r.root, w.default_root);
    }

    #[test]
    fn a_fresh_install_next_to_a_writable_exe_dir_wants_a_portable_root() {
        let w = world();
        let r = resolve_at_startup(&inputs(&w, true));
        assert_eq!(r.root, w.exe_dir.join("LucernaData"));
        assert!(r.must_create);
    }
}

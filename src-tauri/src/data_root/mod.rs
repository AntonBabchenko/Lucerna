//! Data-root location: bootstrap redirect file + resolution + migration.
pub mod blockers;
pub mod cleanup_note;
pub mod migrate;
pub mod plan;
pub mod recovery;
pub mod redirect;
pub mod relocate;
pub mod startup;
pub mod state;
pub mod transient;
pub mod validate;
pub mod walk;

use migrate::Availability;
use redirect::{PointerRead, Redirect};
use std::path::{Path, PathBuf};
use tauri::Manager;

/// Resolved effective data root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The effective root every path derives from.
    pub root: PathBuf,
    /// The user's configured custom root, if any (independent of availability).
    pub configured: Option<PathBuf>,
    /// Why this start is NOT running from the user's data folder. `Some` makes
    /// it a recovery session (see [`DataRoot`]); `root` is then the default
    /// folder AS RESOLVED — the session itself runs on a throwaway root.
    pub fallback: Option<Fallback>,
    /// True when `root` is a portable dir that does not exist yet — the caller
    /// must create it before use (and fall back to the OS default if creation
    /// fails).
    pub must_create: bool,
}

impl Resolved {
    /// The configured data folder could not be used: this is a recovery session.
    pub fn fell_back(&self) -> bool {
        self.fallback.is_some()
    }
}

/// Why the launcher is not running from the user's data folder. The reason
/// travels to the log, the IPC status and the copy: "reconnect it" is a lie
/// about a folder that is plugged in and merely read-only.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fallback {
    /// The pointer names a folder that is not there.
    RootMissing,
    /// The folder is there but cannot be written to (or is not a folder).
    RootNotWritable { details: String },
    /// The folder could not be checked. Same restrictive answer.
    RootUnknown { details: String },
    /// `data-location.json` exists and could not be read.
    PointerUnreadable { details: String },
    /// `data-location.json` was read and is not a usable pointer.
    PointerCorrupt,
}

impl Fallback {
    /// One line for the launcher log.
    pub fn describe(&self) -> String {
        match self {
            Fallback::RootMissing => "the configured folder is not there".into(),
            Fallback::RootNotWritable { details } => {
                format!("the configured folder cannot be written to — {details}")
            }
            Fallback::RootUnknown { details } => {
                format!("the configured folder could not be checked — {details}")
            }
            Fallback::PointerUnreadable { details } => {
                format!("data-location.json could not be read — {details}")
            }
            Fallback::PointerCorrupt => "data-location.json is not a usable pointer".into(),
        }
    }
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
/// first version install (`instances::migrate::migrate_or_seed` guarantees
/// only `app.json` + `instances/`). The JSON check is schema-free on purpose
/// so roots written by any past launcher version keep detecting. There is no
/// marker file — this shape check IS the detector, shared by startup portable
/// adoption (`lib.rs` setup) and the Storage panel's adopt flow
/// (`commands::plan_data_location_change` / `adopt_data_location`).
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
    pointer: PointerRead,
    probe: impl Fn(&Path) -> Availability,
) -> Resolved {
    // Rule 1. A pointer that EXISTS is the user's explicit choice whether or
    // not it can be used: every way of not being able to use it is a recovery
    // session on the default folder, and none of them falls through to the
    // portable rules — the launcher does not switch roots behind the user's back.
    let fall_back = |configured: Option<PathBuf>, reason: Fallback, default: PathBuf| Resolved {
        root: default,
        configured,
        fallback: Some(reason),
        must_create: false,
    };
    match pointer {
        PointerRead::Absent => {}
        PointerRead::Unreadable(details) => {
            return fall_back(None, Fallback::PointerUnreadable { details }, default)
        }
        PointerRead::Corrupt => return fall_back(None, Fallback::PointerCorrupt, default),
        PointerRead::Present(Redirect { path }) => {
            let reason = match probe(&path) {
                Availability::Available => {
                    return Resolved {
                        root: path.clone(),
                        configured: Some(path),
                        fallback: None,
                        must_create: false,
                    }
                }
                Availability::Missing => Fallback::RootMissing,
                Availability::NotADirectory => Fallback::RootNotWritable {
                    details: "not a folder".into(),
                },
                Availability::NotWritable(details) => Fallback::RootNotWritable { details },
                Availability::Unknown(details) => Fallback::RootUnknown { details },
            };
            return fall_back(Some(path), reason, default);
        }
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
                    fallback: None,
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
                    fallback: None,
                    must_create: false,
                };
            }
            PortableState::Absent { creatable } if creatable && !default_has_data => {
                return Resolved {
                    root: candidate.path,
                    configured: None,
                    fallback: None,
                    must_create: true,
                };
            }
            PortableState::Absent { .. } | PortableState::EmptyDir | PortableState::Foreign => {}
        }
    }
    Resolved {
        root: default,
        configured: None,
        fallback: None,
        must_create: false,
    }
}

/// May startup seed (`migrate_or_seed`) and heal (`restore_hidden_app_json`)
/// the root? Never in a recovery session: it has no root of the user's, and a
/// seed written into the default folder is what later blocks "Reset to default".
pub fn should_seed(resolved: &Resolved) -> bool {
    !resolved.fell_back()
}

/// Tauri managed state: where this process keeps things.
///
/// Outside a recovery session all three agree: `root()` is `resolved.root`
/// and so is `launcher_dir`. In a recovery session (`resolved.fell_back()`)
/// the session runs on a throwaway root, so that nothing any code path writes
/// can land in a folder the user's data is ever resolved from, while the
/// launcher's own files stay in the default folder.
pub struct DataRoot {
    /// As resolved. For a fallback `resolved.root` is the default folder —
    /// NOT where this session writes. Use [`DataRoot::root`].
    pub resolved: Resolved,
    /// Where `logs/`, `updates/` and `webview/` live.
    pub launcher_dir: PathBuf,
    /// The throwaway root of a recovery session: `Some` exactly when
    /// `resolved.fell_back()`, WHETHER OR NOT the directory could be created.
    /// A session whose dir could not be created keeps the path anyway — every
    /// write under it fails loudly, and nothing falls through to the default
    /// folder (that second fallback is what used to litter it).
    pub session_root: Option<PathBuf>,
    /// The session dir, when it was actually created. The exit cleanup takes
    /// this and nothing else.
    pub recovery: Option<recovery::RecoveryDir>,
}

impl DataRoot {
    /// A normal session on `resolved.root`.
    pub fn normal(resolved: Resolved) -> Self {
        DataRoot {
            launcher_dir: resolved.root.clone(),
            resolved,
            session_root: None,
            recovery: None,
        }
    }

    /// The root every data path derives from.
    pub fn root(&self) -> &Path {
        self.session_root.as_deref().unwrap_or(&self.resolved.root)
    }
}

/// Integrity chokepoint for every data-creating or launching command. Refuses
/// when this process must not write into — or start anything from — the root
/// it is running on:
///
/// - the configured root is unavailable and we run from the temporary default
///   (`DataLocationUnavailable`): new data would land in the wrong root;
/// - a data-root move is in progress, or has been committed and the launcher
///   has not restarted yet (`DataRelocationInProgress`): this root is being,
///   or has been, emptied.
///
/// The UI gates these actions too, but a direct IPC call, a shortcut's
/// `--launch` arriving through single-instance, or a page reload must not be
/// able to bypass that.
pub fn reject_if_root_unusable(app: &tauri::AppHandle) -> crate::error::Result<()> {
    if app.state::<DataRoot>().resolved.fell_back() {
        return Err(crate::error::Error::DataLocationUnavailable);
    }
    state::global().check_usable()
}

/// Why the data folder can't be opened or measured right now. Travels inside
/// `Error::DataRootUnreachable`, so the page can say which it is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FolderProblem {
    /// Nothing is there (a drive unplugged, a folder removed).
    Missing,
    /// Something is there, but it is not a folder.
    NotAFolder,
    /// A folder is there, but nothing of Lucerna's is in it — neither `app.json`
    /// nor `instances/`. An unmounted mount point is an empty directory of the
    /// parent filesystem. (A DAMAGED data folder — one marker left, or an
    /// unreadable `app.json` — is still the data folder and is not this.)
    NotADataRoot,
    /// It could not be checked; `details` is the OS's own words.
    Unreadable { details: String },
    /// The check got no answer in time (a sleeping or disconnected network drive).
    TimedOut { seconds: u32 },
}

/// Pure: what a stat of the root and a look for Lucerna's own markers say.
/// "Could not tell" (`Unreadable`) is kept apart from "not there" (`Missing`)
/// and from "nothing of ours here" (`NotADataRoot`) at every step.
pub fn data_folder_check(
    is_dir: std::io::Result<bool>,
    has_marker: std::io::Result<bool>,
) -> Result<(), FolderProblem> {
    let unreadable = |e: std::io::Error| FolderProblem::Unreadable {
        details: e.to_string(),
    };
    match is_dir {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(FolderProblem::Missing),
        Err(e) => Err(unreadable(e)),
        Ok(false) => Err(FolderProblem::NotAFolder),
        Ok(true) => match has_marker {
            Ok(true) => Ok(()),
            Ok(false) => Err(FolderProblem::NotADataRoot),
            Err(e) => Err(unreadable(e)),
        },
    }
}

/// Whether any of Lucerna's markers — `app.json`, `instances/` — is in `root`.
/// Presence only: a damaged data folder (a corrupt `app.json`, a deleted
/// `instances/`, a rollback that left `app.json.moved`) is still the data folder,
/// and the one the user most needs to open. A stat that fails otherwise than
/// NotFound is an error, never "absent" (unlike `Path::is_dir`).
pub fn root_has_marker(root: &Path) -> std::io::Result<bool> {
    for name in ["app.json", "instances"] {
        match std::fs::symlink_metadata(root.join(name)) {
            Ok(_) => return Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}

/// The real check of the root, for the commands that open or measure it. A
/// path that answers is not enough: something of Lucerna's must still be there.
pub fn check_root_reachable(root: &Path) -> Result<(), FolderProblem> {
    let is_dir = std::fs::metadata(root).map(|m| m.is_dir());
    let has_marker = if matches!(is_dir, Ok(true)) {
        root_has_marker(root)
    } else {
        Ok(false)
    };
    data_folder_check(is_dir, has_marker)
}

/// Pure: what a stat of a folder we are about to show says, when all that
/// matters is that it is there (the previous data folder after a move — it need
/// not hold anything of ours any more). NotFound → `Missing`; any other error →
/// `Unreadable`, never "not there".
pub fn stat_problem(stat: std::io::Result<()>) -> Option<FolderProblem> {
    match stat {
        Ok(()) => None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(FolderProblem::Missing),
        Err(e) => Some(FolderProblem::Unreadable {
            details: e.to_string(),
        }),
    }
}

/// How to show a folder in the OS file manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenStrategy {
    /// Open the folder itself.
    Open,
    /// Select it in its parent folder.
    Reveal,
}

/// macOS treats a directory named `*.app` as an application bundle: `open`
/// tries to launch it, fails, and still reports success. The default macOS data
/// folder is `…/Application Support/com.lucerna.app`, so there it is revealed.
pub fn folder_open_strategy(path: &Path, os: &str) -> OpenStrategy {
    let is_bundle_name = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"));
    if os == "macos" && is_bundle_name {
        OpenStrategy::Reveal
    } else {
        OpenStrategy::Open
    }
}

#[cfg(test)]
mod folder_tests {
    use super::*;
    use std::io::{Error as IoError, ErrorKind};

    #[test]
    fn nothing_there_is_missing() {
        assert_eq!(
            data_folder_check(Err(IoError::from(ErrorKind::NotFound)), Ok(false)),
            Err(FolderProblem::Missing)
        );
    }

    #[test]
    fn a_stat_that_failed_otherwise_is_could_not_tell_with_the_os_words() {
        let r = data_folder_check(
            Err(IoError::new(ErrorKind::PermissionDenied, "denied!")),
            Ok(false),
        );
        assert!(
            matches!(r, Err(FolderProblem::Unreadable { ref details }) if details.contains("denied!"))
        );
    }

    #[test]
    fn a_file_is_not_a_folder() {
        assert_eq!(
            data_folder_check(Ok(false), Ok(false)),
            Err(FolderProblem::NotAFolder)
        );
    }

    #[test]
    fn an_empty_mount_point_is_not_the_data_folder() {
        assert_eq!(
            data_folder_check(Ok(true), Ok(false)),
            Err(FolderProblem::NotADataRoot)
        );
    }

    #[test]
    fn the_data_folder_is_reachable() {
        assert_eq!(data_folder_check(Ok(true), Ok(true)), Ok(()));
    }

    #[test]
    fn a_marker_that_could_not_be_looked_at_is_could_not_tell_not_not_ours() {
        let r = data_folder_check(
            Ok(true),
            Err(IoError::new(ErrorKind::PermissionDenied, "no!")),
        );
        assert!(
            matches!(r, Err(FolderProblem::Unreadable { ref details }) if details.contains("no!"))
        );
    }

    #[test]
    fn a_folder_that_is_gone_is_missing_and_one_that_could_not_be_checked_is_not() {
        assert_eq!(stat_problem(Ok(())), None);
        assert_eq!(
            stat_problem(Err(IoError::from(ErrorKind::NotFound))),
            Some(FolderProblem::Missing)
        );
        assert!(matches!(
            stat_problem(Err(IoError::new(ErrorKind::PermissionDenied, "nope"))),
            Some(FolderProblem::Unreadable { ref details }) if details.contains("nope")
        ));
    }

    #[test]
    fn a_damaged_data_folder_is_still_the_data_folder() {
        // The folder the user most needs to open after a bad rollback or a hand
        // edit must not be called a disconnected drive.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("app.json"), "not json at all").expect("app.json");
        assert_eq!(check_root_reachable(dir.path()), Ok(()));
        let only_instances = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(only_instances.path().join("instances")).expect("instances");
        assert_eq!(check_root_reachable(only_instances.path()), Ok(()));
    }

    #[test]
    fn check_root_reachable_reads_the_real_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            check_root_reachable(&dir.path().join("gone")),
            Err(FolderProblem::Missing)
        );
        assert_eq!(
            check_root_reachable(dir.path()),
            Err(FolderProblem::NotADataRoot)
        );
        std::fs::create_dir(dir.path().join("instances")).expect("instances");
        std::fs::write(dir.path().join("app.json"), "{}").expect("app.json");
        assert_eq!(check_root_reachable(dir.path()), Ok(()));
    }

    #[test]
    fn a_macos_app_named_folder_is_revealed_not_opened() {
        let root = Path::new("/Users/u/Library/Application Support/com.lucerna.app");
        assert_eq!(folder_open_strategy(root, "macos"), OpenStrategy::Reveal);
        assert_eq!(
            folder_open_strategy(Path::new("/Volumes/X/Lucerna.APP"), "macos"),
            OpenStrategy::Reveal
        );
    }

    #[test]
    fn every_other_folder_is_opened() {
        assert_eq!(
            folder_open_strategy(Path::new("/Volumes/X/LucernaData"), "macos"),
            OpenStrategy::Open
        );
        assert_eq!(
            folder_open_strategy(
                Path::new(r"C:\Users\u\AppData\Roaming\com.lucerna.app"),
                "windows"
            ),
            OpenStrategy::Open
        );
        assert_eq!(
            folder_open_strategy(Path::new("/home/u/.local/share/com.lucerna.app"), "linux"),
            OpenStrategy::Open
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def() -> PathBuf {
        PathBuf::from("/default")
    }

    /// The call shape the rule tests below were written against (an optional
    /// pointer, a yes / no probe). It shadows the glob import on purpose, so
    /// their bodies — the design authority for rules 1–5 — stay as they were.
    /// The reason-carrying cases call `super::resolve_root` directly.
    fn resolve_root(
        default: PathBuf,
        default_has_data: bool,
        portable: Option<PortableCandidate>,
        redirect: Option<Redirect>,
        available: impl Fn(&Path) -> bool,
    ) -> Resolved {
        super::resolve_root(
            default,
            default_has_data,
            portable,
            redirect.map_or(PointerRead::Absent, PointerRead::Present),
            |p| {
                if available(p) {
                    Availability::Available
                } else {
                    Availability::Missing
                }
            },
        )
    }

    fn custom() -> PointerRead {
        PointerRead::Present(Redirect {
            path: PathBuf::from("/custom"),
        })
    }

    #[test]
    fn an_unreadable_pointer_is_a_fallback_not_a_missing_pointer() {
        let r = super::resolve_root(
            def(),
            true,
            None,
            PointerRead::Unreadable("access denied".into()),
            |_| Availability::Available,
        );
        assert_eq!(r.root, def());
        assert_eq!(r.configured, None);
        assert_eq!(
            r.fallback,
            Some(Fallback::PointerUnreadable {
                details: "access denied".into()
            })
        );
    }

    #[test]
    fn a_corrupt_pointer_is_a_fallback_and_has_no_configured_path() {
        let r = super::resolve_root(def(), false, None, PointerRead::Corrupt, |_| {
            Availability::Available
        });
        assert_eq!(r.fallback, Some(Fallback::PointerCorrupt));
        assert!(r.fell_back() && r.configured.is_none() && !r.must_create);
    }

    #[test]
    fn a_pointer_fallback_ignores_the_exe_side_root() {
        // Same rationale as an unavailable folder: the user's explicit choice
        // keeps priority, the launcher does not switch roots behind their back.
        for pointer in [PointerRead::Corrupt, PointerRead::Unreadable("io".into())] {
            let r =
                super::resolve_root(def(), false, portable(PortableState::Root), pointer, |_| {
                    Availability::Available
                });
            assert_eq!(r.root, def());
            assert!(r.fell_back());
        }
    }

    #[test]
    fn each_probe_answer_keeps_its_reason() {
        let cases = [
            (Availability::Missing, Fallback::RootMissing),
            (
                Availability::NotADirectory,
                Fallback::RootNotWritable {
                    details: "not a folder".into(),
                },
            ),
            (
                Availability::NotWritable("read-only".into()),
                Fallback::RootNotWritable {
                    details: "read-only".into(),
                },
            ),
            (
                Availability::Unknown("stat failed".into()),
                Fallback::RootUnknown {
                    details: "stat failed".into(),
                },
            ),
        ];
        for (answer, reason) in cases {
            let r = super::resolve_root(def(), false, None, custom(), |_| answer.clone());
            assert_eq!(r.root, def());
            assert_eq!(r.configured, Some(PathBuf::from("/custom")));
            assert_eq!(r.fallback, Some(reason));
        }
    }

    #[test]
    fn a_recovery_session_is_never_seeded() {
        let normal = super::resolve_root(def(), false, None, PointerRead::Absent, |_| {
            Availability::Available
        });
        assert!(should_seed(&normal));
        let recovery = super::resolve_root(def(), false, None, custom(), |_| Availability::Missing);
        assert!(!should_seed(&recovery));
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
        assert!(!r.fell_back() && r.configured.is_none() && !r.must_create);
    }

    #[test]
    fn available_custom_is_used() {
        let red = Some(Redirect {
            path: PathBuf::from("/custom"),
        });
        let r = resolve_root(def(), false, None, red, |_| true);
        assert_eq!(r.root, PathBuf::from("/custom"));
        assert_eq!(r.configured, Some(PathBuf::from("/custom")));
        assert!(!r.fell_back() && !r.must_create);
    }

    #[test]
    fn unavailable_custom_falls_back_but_keeps_configured() {
        let red = Some(Redirect {
            path: PathBuf::from("/custom"),
        });
        let r = resolve_root(def(), false, None, red, |_| false);
        assert_eq!(r.root, def());
        assert_eq!(r.configured, Some(PathBuf::from("/custom")));
        assert!(r.fell_back() && !r.must_create);
    }

    #[test]
    fn root_shaped_exe_side_dir_is_adopted() {
        let r = resolve_root(def(), false, portable(PortableState::Root), None, |_| true);
        assert_eq!(r.root, PathBuf::from("/install/LucernaData"));
        assert!(r.configured.is_none() && !r.fell_back() && !r.must_create);
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
        assert!(r.fell_back());
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

    #[test]
    fn looks_like_data_root_rejects_remaining_shapes() {
        // Complements the test above with the negatives ported from the
        // (removed) plan::is_data_root twin: app.json without instances/,
        // `instances` as a FILE, and a missing dir entirely.
        let t = tempfile::tempdir().unwrap();
        assert!(
            !looks_like_data_root(&t.path().join("nope")),
            "missing dir is not a root"
        );

        let only_app = t.path().join("only-app");
        std::fs::create_dir_all(&only_app).unwrap();
        std::fs::write(only_app.join("app.json"), "{}").unwrap();
        assert!(
            !looks_like_data_root(&only_app),
            "app.json without instances/ is not a root"
        );

        let file_instances = t.path().join("file-instances");
        std::fs::create_dir_all(&file_instances).unwrap();
        std::fs::write(file_instances.join("instances"), b"").unwrap();
        std::fs::write(file_instances.join("app.json"), "{}").unwrap();
        assert!(
            !looks_like_data_root(&file_instances),
            "`instances` as a file is not a root"
        );
    }
}

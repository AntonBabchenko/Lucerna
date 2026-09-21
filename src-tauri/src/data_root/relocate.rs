//! The data-root move pipeline. No `AppHandle`: everything that needs one is a
//! hook, and the filesystem mutations a test must be able to fail are `Io`.
//!
//! Order (spec 2026-09-21 §4.2-4.3):
//!   snapshot target → copy (all but app.json) → verify → still safe? →
//!   copy+verify app.json → SWITCH { commit pointer → hide old app.json →
//!   does the next start land on the target? } → delete the old root.
//!
//! Before the switch nothing in `current` is touched, so every failure there
//! is a `NotSwitched` whose cleanup removes only what this run created. The
//! source is deleted only after the landing was confirmed; otherwise the
//! switch is rolled back, each step checked.

use crate::data_root::transient::{self, EntryRule, APP_JSON};
use crate::data_root::walk::{self, Walk, WalkStop};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// What the old root's `app.json` is renamed to once the pointer is committed.
/// Without an `app.json` the old root stops being root-shaped
/// (`looks_like_data_root`), so neither an adopt offer nor the startup
/// adoption of `<exe dir>\LucernaData` can pick it up again. Reversible.
pub const HIDDEN_APP_JSON: &str = "app.json.moved";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Copying,
    Verifying,
    Switching,
    Deleting,
}

pub struct Io<'a> {
    pub list_dir: &'a dyn Fn(&Path) -> std::io::Result<Vec<PathBuf>>,
    pub copy_file: &'a dyn Fn(&Path, &Path) -> std::io::Result<u64>,
    /// Remove a file, or a directory with everything in it.
    pub remove_entry: &'a dyn Fn(&Path) -> std::io::Result<()>,
    pub rename_entry: &'a dyn Fn(&Path, &Path) -> std::io::Result<()>,
}

pub fn real_remove_entry(path: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

pub fn real_rename_entry(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

impl Io<'static> {
    pub fn real() -> Self {
        Io {
            list_dir: &walk::real_list_dir,
            copy_file: &walk::real_copy_file,
            remove_entry: &real_remove_entry,
            rename_entry: &real_rename_entry,
        }
    }
}

pub struct Hooks<'a> {
    /// `(phase, bytes copied so far)`. Called at every phase change and after
    /// every copied file.
    pub on_progress: &'a mut dyn FnMut(Phase, u64),
    pub is_cancelled: &'a dyn Fn() -> bool,
    /// Asked once, after verification: did a game, server or long operation
    /// start while we were copying?
    pub still_safe_to_switch: &'a dyn Fn() -> bool,
    /// Point the bootstrap redirect at the target (or remove it on a reset).
    pub commit_pointer: &'a mut dyn FnMut() -> Result<(), String>,
    /// Put the redirect back exactly as it was before `commit_pointer`.
    pub rollback_pointer: &'a mut dyn FnMut() -> Result<(), String>,
    /// Would the NEXT start resolve to the target, and is the target still an
    /// available, complete root? Anything but a confident yes is a no.
    pub lands_on_target: &'a dyn Fn() -> bool,
    /// The switch held; the old root is about to be deleted.
    pub on_switched: &'a mut dyn FnMut(),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Everything moved and the old root is empty (bar launcher-owned entries).
    Moved,
    /// The move is valid and committed. `entries` are top-level names still in
    /// the old root. `old_root_intact` = nothing was deleted at all.
    MovedWithLeftovers {
        entries: Vec<String>,
        old_root_intact: bool,
    },
    /// Cancelled before the switch; the target was cleaned up.
    Cancelled,
}

/// The move did not happen: the launcher still runs from, and will restart
/// into, `current`.
#[derive(Debug, PartialEq, Eq)]
pub struct NotSwitched {
    pub phase: Phase,
    pub reason: String,
    /// The cleanup of the target failed; something is left at this path.
    pub partial_copy_left: Option<PathBuf>,
    /// A rollback step failed; `reason` says what is left in which state.
    pub restore_incomplete: bool,
}

enum Why {
    Cancelled,
    Failed(String),
}

impl From<WalkStop> for Why {
    fn from(stop: WalkStop) -> Self {
        match stop {
            WalkStop::Cancelled => Why::Cancelled,
            WalkStop::Failed(e) => Why::Failed(e.to_string()),
        }
    }
}

/// What `target` held before this run touched it.
struct TargetBefore {
    existed: bool,
    names: HashSet<OsString>,
}

impl TargetBefore {
    fn snapshot(target: &Path, io: &Io<'_>) -> Result<Self, String> {
        match (io.list_dir)(target) {
            Ok(children) => Ok(Self {
                existed: true,
                names: children
                    .iter()
                    .filter_map(|c| c.file_name().map(OsString::from))
                    .collect(),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                existed: false,
                names: HashSet::new(),
            }),
            Err(e) => Err(format!("{}: {e}", target.display())),
        }
    }
}

fn remove_tolerating_absence(io: &Io<'_>, path: &Path) -> Result<(), String> {
    match (io.remove_entry)(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Remove what THIS run created in `target`. `app.json` goes first, by direct
/// path, so that whatever else fails the remnant is never root-shaped.
fn clean_target(target: &Path, before: &TargetBefore, io: &Io<'_>) -> Result<(), String> {
    if !before.names.contains(&OsString::from(APP_JSON)) {
        remove_tolerating_absence(io, &target.join(APP_JSON))?;
    }
    if !before.existed {
        return remove_tolerating_absence(io, target);
    }
    let children = match (io.list_dir)(target) {
        Ok(children) => children,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("{}: {e}", target.display())),
    };
    for child in children {
        let created_by_us = child
            .file_name()
            .is_some_and(|name| !before.names.contains(name));
        if created_by_us {
            remove_tolerating_absence(io, &child)?;
        }
    }
    Ok(())
}

/// Give up before the switch: clean the target, then say what happened.
fn abort(
    why: Why,
    phase: Phase,
    target: &Path,
    before: &TargetBefore,
    io: &Io<'_>,
) -> Result<Outcome, NotSwitched> {
    match (why, clean_target(target, before, io)) {
        (Why::Cancelled, Ok(())) => Ok(Outcome::Cancelled),
        (Why::Cancelled, Err(detail)) => Err(NotSwitched {
            phase,
            reason: format!(
                "the move was cancelled, but the partial copy could not be removed: {detail}"
            ),
            partial_copy_left: Some(target.to_path_buf()),
            restore_incomplete: false,
        }),
        (Why::Failed(reason), Ok(())) => Err(NotSwitched {
            phase,
            reason,
            partial_copy_left: None,
            restore_incomplete: false,
        }),
        (Why::Failed(reason), Err(detail)) => Err(NotSwitched {
            phase,
            reason: format!("{reason}; the partial copy could not be removed: {detail}"),
            partial_copy_left: Some(target.to_path_buf()),
            restore_incomplete: false,
        }),
    }
}

/// Copy `app.json` — the file that makes the target root-shaped — and verify it.
fn copy_app_json(current: &Path, target: &Path, io: &Io<'_>) -> Result<(), String> {
    let from = current.join(APP_JSON);
    let to = target.join(APP_JSON);
    let from_len = std::fs::metadata(&from)
        .map_err(|e| format!("{}: {e}", from.display()))?
        .len();
    (io.copy_file)(&from, &to).map_err(|e| format!("{}: {e}", to.display()))?;
    let to_len = std::fs::metadata(&to)
        .map_err(|e| format!("{}: {e}", to.display()))?
        .len();
    if from_len != to_len {
        return Err(format!(
            "verification failed for {}: source is {from_len} bytes but the copy is {to_len} bytes",
            from.display()
        ));
    }
    Ok(())
}

/// The next start would NOT land on the target: undo the switch, each step
/// checked, and report exactly what could not be restored.
fn roll_back(
    current: &Path,
    target: &Path,
    before: &TargetBefore,
    io: &Io<'_>,
    hooks: &mut Hooks<'_>,
    old_root_was_hidden: bool,
) -> NotSwitched {
    let mut problems = Vec::new();
    if old_root_was_hidden {
        if let Err(e) = (io.rename_entry)(&current.join(HIDDEN_APP_JSON), &current.join(APP_JSON)) {
            problems.push(format!(
                "{} is still named {HIDDEN_APP_JSON} — rename it back to {APP_JSON}: {e}",
                current.join(HIDDEN_APP_JSON).display()
            ));
        }
    }
    if let Err(e) = (hooks.rollback_pointer)() {
        problems.push(format!(
            "the data-folder setting could not be restored: {e}"
        ));
    }
    let partial_copy_left = clean_target(target, before, io)
        .err()
        .map(|_| target.to_path_buf());
    let headline = "Lucerna could not confirm that the next start would use the new folder";
    NotSwitched {
        phase: Phase::Switching,
        reason: if problems.is_empty() {
            headline.to_string()
        } else {
            format!("{headline}; {}", problems.join("; "))
        },
        partial_copy_left,
        restore_incomplete: !problems.is_empty(),
    }
}

/// Remove one top-level entry of the old root. `Some(name)` = it is still there.
fn remove_old_entry(child: &Path, canonical_target: &Path, io: &Io<'_>) -> Option<String> {
    let name = child.file_name()?;
    if transient::is_skipped_top_level(name) {
        return None;
    }
    let leftover = Some(name.to_string_lossy().into_owned());
    // Defence in depth: never delete an entry that is, or contains, the target.
    // Same/nested targets were rejected before the copy; this holds under any
    // path-spelling edge case. "Could not tell" keeps the entry.
    let canonical_child = match std::fs::canonicalize(child) {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => return leftover,
    };
    if canonical_target == canonical_child || canonical_target.starts_with(&canonical_child) {
        return leftover;
    }
    match (io.remove_entry)(child) {
        Ok(()) => None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            crate::diag!("[data-move] could not remove {}: {e}", child.display());
            leftover
        }
    }
}

/// Delete the old root's top-level entries. `None` = nothing was deleted
/// (the target could not be canonicalized, or the old root not enumerated).
fn delete_old_root(current: &Path, target: &Path, io: &Io<'_>) -> Option<Vec<String>> {
    let canonical_target = match std::fs::canonicalize(target) {
        Ok(p) => p,
        Err(e) => {
            crate::diag!(
                "[data-move] old data kept: cannot canonicalize {}: {e}",
                target.display()
            );
            return None;
        }
    };
    let children = match (io.list_dir)(current) {
        Ok(children) => children,
        Err(e) => {
            crate::diag!(
                "[data-move] old data kept: cannot enumerate {}: {e}",
                current.display()
            );
            return None;
        }
    };
    Some(
        children
            .iter()
            .filter_map(|child| remove_old_entry(child, &canonical_target, io))
            .collect(),
    )
}

pub fn relocate(
    current: &Path,
    target: &Path,
    io: &Io<'_>,
    hooks: &mut Hooks<'_>,
) -> Result<Outcome, NotSwitched> {
    let refuse = |reason: String| NotSwitched {
        phase: Phase::Copying,
        reason,
        partial_copy_left: None,
        restore_incomplete: false,
    };
    if let Err(e) = std::fs::metadata(current.join(APP_JSON)) {
        return Err(refuse(format!(
            "the current data folder has no readable {APP_JSON}: {e}"
        )));
    }
    let before = TargetBefore::snapshot(target, io).map_err(refuse)?;

    // Phases 2-3: everything except app.json, which would make the target
    // root-shaped before it is complete and verified.
    let rule = |rel: &Path| {
        if rel == Path::new(APP_JSON) {
            EntryRule::Skip
        } else {
            transient::classify(rel)
        }
    };
    let walk = Walk {
        rule: &rule,
        list_dir: io.list_dir,
        copy_file: io.copy_file,
        cancelled: hooks.is_cancelled,
    };
    let mut copied = 0u64;
    (hooks.on_progress)(Phase::Copying, 0);
    let copy_result = {
        let on_progress = &mut *hooks.on_progress;
        walk::copy_tree(
            current,
            target,
            &walk,
            &mut |bytes| on_progress(Phase::Copying, bytes),
            &mut copied,
        )
    };
    if let Err(stop) = copy_result {
        return abort(stop.into(), Phase::Copying, target, &before, io);
    }
    (hooks.on_progress)(Phase::Verifying, copied);
    if let Err(stop) = walk::verify_tree(current, target, &walk) {
        return abort(stop.into(), Phase::Verifying, target, &before, io);
    }
    if !(hooks.still_safe_to_switch)() {
        let why = Why::Failed(
            "a game, a server or another operation started while the data was being copied"
                .to_string(),
        );
        return abort(why, Phase::Verifying, target, &before, io);
    }

    // Phases 5-6: the switch. From the commit on there is no cancel.
    (hooks.on_progress)(Phase::Switching, copied);
    if let Err(reason) = copy_app_json(current, target, io) {
        return abort(Why::Failed(reason), Phase::Switching, target, &before, io);
    }
    if let Err(reason) = (hooks.commit_pointer)() {
        let why = Why::Failed(format!(
            "the data-folder setting could not be updated: {reason}"
        ));
        return abort(why, Phase::Switching, target, &before, io);
    }
    let hidden = (io.rename_entry)(&current.join(APP_JSON), &current.join(HIDDEN_APP_JSON));
    if !(hooks.lands_on_target)() {
        return Err(roll_back(
            current,
            target,
            &before,
            io,
            hooks,
            hidden.is_ok(),
        ));
    }
    if let Err(e) = hidden {
        // The redirect is authoritative, so the move holds — but the old root
        // is still root-shaped. Delete nothing: a gutted folder that still
        // looks like a data root is worse than a complete duplicate.
        crate::diag!(
            "[data-move] old data kept: could not hide {}: {e}",
            current.join(APP_JSON).display()
        );
        return Ok(Outcome::MovedWithLeftovers {
            entries: Vec::new(),
            old_root_intact: true,
        });
    }

    // Phase 7.
    (hooks.on_switched)();
    (hooks.on_progress)(Phase::Deleting, copied);
    Ok(match delete_old_root(current, target, io) {
        None => Outcome::MovedWithLeftovers {
            entries: Vec::new(),
            old_root_intact: true,
        },
        Some(leftovers) if leftovers.is_empty() => Outcome::Moved,
        Some(leftovers) => Outcome::MovedWithLeftovers {
            entries: leftovers,
            old_root_intact: false,
        },
    })
}

/// Try again to remove `names` (top-level entries of `old_root`) after a move
/// that left them behind. Returns the names that are STILL there.
pub fn retry_leftovers(
    old_root: &Path,
    target: &Path,
    names: &[String],
    io: &Io<'_>,
) -> Vec<String> {
    let Ok(canonical_target) = std::fs::canonicalize(target) else {
        return names.to_vec();
    };
    names
        .iter()
        .filter_map(|name| remove_old_entry(&old_root.join(name), &canonical_target, io))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_root::looks_like_data_root;
    use std::cell::{Cell, RefCell};
    use tempfile::{tempdir, TempDir};

    #[derive(Clone, Copy)]
    enum AfterCopy {
        TruncateTarget,
        GrowSource,
    }

    /// Scripted faults. Every field defaults to "behave".
    #[derive(Default)]
    struct Script {
        cancel_after_files: Option<usize>,
        unsafe_to_switch: bool,
        commit_fails: bool,
        rollback_fails: bool,
        does_not_land: bool,
        fail_hide: bool,
        fail_unhide: bool,
        fail_copy_of: Option<&'static str>,
        fail_remove_of: Option<&'static str>,
        fail_list_of: Option<&'static str>,
        after_copy_of: Option<(&'static str, AfterCopy)>,
        app_json_listed_first: bool,
    }

    fn name_of(p: &Path) -> String {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn scripted_error(what: &str) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, what.to_string())
    }

    /// A realistic root: shape markers, shared data, the live log, the profile.
    fn seed_root(root: &Path) {
        for (rel, bytes) in [
            ("app.json", b"{}".as_slice()),
            ("account.json", b"{\"accounts\":[]}"),
            ("instances/Default/instance.json", b"{\"id\":\"Default\"}"),
            (
                "instances/Default/.minecraft/saves/World/level.dat",
                b"WORLDDATA",
            ),
            ("libraries/a.jar", b"LIBRARY"),
            ("logs/lucerna.log", b"line-1\n"),
            ("webview/EBWebView/Local State", b"{}"),
        ] {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }
    }

    /// Sorted relative paths of every FILE under `root`.
    fn listing(root: &Path) -> Vec<String> {
        fn walk_dir(base: &Path, dir: &Path, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk_dir(base, &path, out);
                } else {
                    out.push(
                        path.strip_prefix(base)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        let mut out = Vec::new();
        walk_dir(root, root, &mut out);
        out.sort();
        out
    }

    struct Rig {
        _dir: TempDir,
        current: PathBuf,
        target: PathBuf,
    }

    fn rig() -> Rig {
        let dir = tempdir().unwrap();
        let current = dir.path().join("com.lucerna.app");
        let target = dir.path().join("Games").join("LucernaData");
        std::fs::create_dir_all(dir.path().join("Games")).unwrap();
        seed_root(&current);
        Rig {
            _dir: dir,
            current,
            target,
        }
    }

    /// Run the pipeline under `script`; return its result and the trace.
    fn run(
        current: &Path,
        target: &Path,
        s: &Script,
    ) -> (Result<Outcome, NotSwitched>, Vec<String>) {
        let trace: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let copied_files = Cell::new(0usize);
        let note = |event: String| trace.borrow_mut().push(event);
        // Spec §4.4: until app.json is copied (after verification), the target
        // must never look like a data root — whatever interrupts the move.
        let assert_not_adoptable_yet = |at: &str| {
            assert!(
                !looks_like_data_root(target),
                "the target is root-shaped before the switch ({at})"
            );
        };

        let list_dir = |dir: &Path| -> std::io::Result<Vec<PathBuf>> {
            if s.fail_list_of.is_some_and(|n| name_of(dir) == n) {
                return Err(scripted_error("scripted: unreadable directory"));
            }
            let mut children = walk::real_list_dir(dir)?;
            if s.app_json_listed_first {
                children.sort_by_key(|p| name_of(p) != APP_JSON);
            }
            Ok(children)
        };
        let copy_file = |from: &Path, to: &Path| -> std::io::Result<u64> {
            assert_not_adoptable_yet("copy_file");
            if s.fail_copy_of.is_some_and(|n| name_of(from) == n) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "scripted: disk full",
                ));
            }
            note(format!("copy:{}", name_of(from)));
            copied_files.set(copied_files.get() + 1);
            let bytes = std::fs::copy(from, to)?;
            match s.after_copy_of {
                Some((n, AfterCopy::TruncateTarget)) if name_of(from) == n => {
                    std::fs::write(to, b"").unwrap();
                }
                Some((n, AfterCopy::GrowSource)) if name_of(from) == n => {
                    let mut grown = std::fs::read(from).unwrap();
                    grown.extend_from_slice(b"appended-after-the-copy\n");
                    std::fs::write(from, grown).unwrap();
                }
                _ => {}
            }
            Ok(bytes)
        };
        let remove_entry = |p: &Path| -> std::io::Result<()> {
            if s.fail_remove_of.is_some_and(|n| name_of(p) == n) {
                return Err(scripted_error("scripted: in use"));
            }
            note(format!("remove:{}", name_of(p)));
            real_remove_entry(p)
        };
        let rename_entry = |from: &Path, to: &Path| -> std::io::Result<()> {
            let hiding = name_of(to) == HIDDEN_APP_JSON;
            if (hiding && s.fail_hide) || (!hiding && s.fail_unhide) {
                return Err(scripted_error("scripted: rename refused"));
            }
            note(format!("rename:{}->{}", name_of(from), name_of(to)));
            std::fs::rename(from, to)
        };
        let io = Io {
            list_dir: &list_dir,
            copy_file: &copy_file,
            remove_entry: &remove_entry,
            rename_entry: &rename_entry,
        };

        let mut on_progress = |phase: Phase, _bytes: u64| {
            // Record each phase once, at its first report.
            let tag = format!("phase:{phase:?}");
            let seen = trace.borrow().iter().any(|e| e == &tag);
            if !seen {
                trace.borrow_mut().push(tag);
            }
        };
        let is_cancelled = || {
            s.cancel_after_files
                .is_some_and(|n| copied_files.get() >= n)
        };
        let still_safe = || {
            assert_not_adoptable_yet("still_safe_to_switch");
            !s.unsafe_to_switch
        };
        let mut commit = || {
            assert!(
                looks_like_data_root(target),
                "the pointer was committed to a target that is not a complete root"
            );
            note("commit".into());
            if s.commit_fails {
                Err("scripted: redirect not writable".to_string())
            } else {
                Ok(())
            }
        };
        let mut rollback = || {
            note("rollback".into());
            if s.rollback_fails {
                Err("scripted: redirect not restorable".to_string())
            } else {
                Ok(())
            }
        };
        let lands = || {
            note("lands?".into());
            !s.does_not_land
        };
        let mut on_switched = || note("switched".into());
        let mut hooks = Hooks {
            on_progress: &mut on_progress,
            is_cancelled: &is_cancelled,
            still_safe_to_switch: &still_safe,
            commit_pointer: &mut commit,
            rollback_pointer: &mut rollback,
            lands_on_target: &lands,
            on_switched: &mut on_switched,
        };

        let result = relocate(current, target, &io, &mut hooks);
        let events = trace.borrow().clone();
        (result, events)
    }

    fn position(events: &[String], needle: &str) -> usize {
        events
            .iter()
            .position(|e| e == needle)
            .unwrap_or_else(|| panic!("`{needle}` not in the trace: {events:?}"))
    }

    fn first_source_removal(events: &[String]) -> usize {
        events
            .iter()
            .position(|e| e.starts_with("remove:"))
            .unwrap_or_else(|| panic!("no removal in the trace: {events:?}"))
    }

    #[test]
    fn a_clean_move_copies_verifies_switches_and_only_then_deletes() {
        let r = rig();
        let (result, events) = run(&r.current, &r.target, &Script::default());
        assert_eq!(result, Ok(Outcome::Moved), "{events:?}");

        // Order: every copy, app.json last → commit → hide → landing check → delete.
        let copies: Vec<&String> = events.iter().filter(|e| e.starts_with("copy:")).collect();
        assert_eq!(copies.last().map(|s| s.as_str()), Some("copy:app.json"));
        let commit = position(&events, "commit");
        let hide = position(&events, "rename:app.json->app.json.moved");
        let lands = position(&events, "lands?");
        let switched = position(&events, "switched");
        assert!(position(&events, "copy:app.json") < commit);
        assert!(commit < hide && hide < lands && lands < switched);
        assert!(switched < first_source_removal(&events));
        for phase in [
            "phase:Copying",
            "phase:Verifying",
            "phase:Switching",
            "phase:Deleting",
        ] {
            position(&events, phase);
        }

        // The target is a complete root without the launcher-owned entries.
        assert!(looks_like_data_root(&r.target));
        assert_eq!(
            listing(&r.target),
            vec![
                "account.json",
                "app.json",
                "instances/Default/.minecraft/saves/World/level.dat",
                "instances/Default/instance.json",
                "libraries/a.jar",
                "logs/lucerna.log",
            ]
        );
        // The old root keeps only the running profile.
        assert_eq!(listing(&r.current), vec!["webview/EBWebView/Local State"]);
    }

    #[test]
    fn a_removal_failure_after_the_switch_is_a_leftover_not_an_error() {
        let r = rig();
        let script = Script {
            fail_remove_of: Some("libraries"),
            ..Script::default()
        };
        let (result, events) = run(&r.current, &r.target, &script);
        assert_eq!(
            result,
            Ok(Outcome::MovedWithLeftovers {
                entries: vec!["libraries".to_string()],
                old_root_intact: false,
            }),
            "{events:?}"
        );
        // STOR-01: the pointer was committed and the old root hidden BEFORE any delete.
        assert!(position(&events, "commit") < first_source_removal(&events));
        assert!(
            position(&events, "rename:app.json->app.json.moved") < first_source_removal(&events)
        );
        // Everything else was still removed; the failing entry and the profile stay.
        assert_eq!(
            listing(&r.current),
            vec!["libraries/a.jar", "webview/EBWebView/Local State"]
        );
        assert!(!looks_like_data_root(&r.current));
    }

    #[test]
    fn a_failed_pointer_commit_touches_nothing_in_the_source() {
        let r = rig();
        let before = listing(&r.current);
        let script = Script {
            commit_fails: true,
            ..Script::default()
        };
        let (result, events) = run(&r.current, &r.target, &script);
        let failure = result.expect_err("the move must not happen");
        assert_eq!(failure.phase, Phase::Switching);
        assert_eq!(failure.partial_copy_left, None);
        assert!(!failure.restore_incomplete);
        assert!(
            !events.iter().any(|e| e.starts_with("rename:")),
            "{events:?}"
        );
        assert!(!events.iter().any(|e| e == "lands?"), "{events:?}");
        assert_eq!(listing(&r.current), before);
        assert!(!r.target.exists(), "the copy is removed again");
    }

    #[test]
    fn when_the_next_start_would_not_land_on_the_target_everything_is_rolled_back() {
        let r = rig();
        let before = listing(&r.current);
        let script = Script {
            does_not_land: true,
            ..Script::default()
        };
        let (result, events) = run(&r.current, &r.target, &script);
        let failure = result.expect_err("the move must not happen");
        assert_eq!(failure.phase, Phase::Switching);
        assert!(!failure.restore_incomplete, "{failure:?}");
        assert_eq!(failure.partial_copy_left, None);
        position(&events, "rename:app.json.moved->app.json");
        position(&events, "rollback");
        assert!(!events.iter().any(|e| e == "switched"), "{events:?}");
        assert_eq!(
            listing(&r.current),
            before,
            "the source is exactly as it was"
        );
        assert!(looks_like_data_root(&r.current));
        assert!(!r.target.exists());
    }

    #[test]
    fn a_failed_rollback_step_is_reported_as_an_incomplete_restore() {
        for script in [
            Script {
                does_not_land: true,
                rollback_fails: true,
                ..Script::default()
            },
            Script {
                does_not_land: true,
                fail_unhide: true,
                ..Script::default()
            },
        ] {
            let r = rig();
            let (result, _) = run(&r.current, &r.target, &script);
            let failure = result.expect_err("the move must not happen");
            assert!(failure.restore_incomplete, "{failure:?}");
            assert!(
                listing(&r.current).contains(&"libraries/a.jar".to_string()),
                "no data was deleted"
            );
            if script.fail_unhide {
                assert!(
                    failure.reason.contains(HIDDEN_APP_JSON),
                    "{}",
                    failure.reason
                );
            }
        }
    }

    #[test]
    fn when_hiding_the_old_root_fails_but_the_switch_holds_nothing_is_deleted() {
        let r = rig();
        let before = listing(&r.current);
        let script = Script {
            fail_hide: true,
            ..Script::default()
        };
        let (result, events) = run(&r.current, &r.target, &script);
        assert_eq!(
            result,
            Ok(Outcome::MovedWithLeftovers {
                entries: vec![],
                old_root_intact: true,
            }),
            "{events:?}"
        );
        assert!(
            !events.iter().any(|e| e.starts_with("remove:")),
            "{events:?}"
        );
        assert_eq!(listing(&r.current), before);
        assert!(looks_like_data_root(&r.target));
    }

    #[test]
    fn a_copy_failure_cleans_the_target_and_leaves_the_pointer_alone() {
        let r = rig();
        let before = listing(&r.current);
        let script = Script {
            fail_copy_of: Some("a.jar"),
            ..Script::default()
        };
        let (result, events) = run(&r.current, &r.target, &script);
        let failure = result.expect_err("the move must not happen");
        assert_eq!(failure.phase, Phase::Copying);
        assert!(failure.reason.contains("disk full"), "{}", failure.reason);
        assert_eq!(failure.partial_copy_left, None);
        assert!(!events.iter().any(|e| e == "commit"), "{events:?}");
        assert_eq!(listing(&r.current), before);
        assert!(!r.target.exists());
    }

    #[test]
    fn a_failed_cleanup_is_reported_and_the_remnant_is_never_adoptable() {
        // Copy failure, verify failure and cancel — each with a cleanup that
        // cannot remove the target (STOR-02 + audit G-3).
        for script in [
            Script {
                fail_copy_of: Some("a.jar"),
                fail_remove_of: Some("LucernaData"),
                ..Script::default()
            },
            Script {
                after_copy_of: Some(("a.jar", AfterCopy::TruncateTarget)),
                fail_remove_of: Some("LucernaData"),
                ..Script::default()
            },
            Script {
                cancel_after_files: Some(2),
                fail_remove_of: Some("LucernaData"),
                ..Script::default()
            },
        ] {
            let r = rig();
            let (result, events) = run(&r.current, &r.target, &script);
            let failure = result.expect_err("a failed cleanup is never a clean outcome");
            assert_eq!(
                failure.partial_copy_left,
                Some(r.target.clone()),
                "{events:?}"
            );
            assert!(r.target.exists());
            assert!(!looks_like_data_root(&r.target), "{events:?}");
            assert!(!events.iter().any(|e| e == "commit"), "{events:?}");
        }
    }

    #[test]
    fn a_verification_failure_is_not_a_move() {
        let r = rig();
        let script = Script {
            after_copy_of: Some(("a.jar", AfterCopy::TruncateTarget)),
            ..Script::default()
        };
        let (result, events) = run(&r.current, &r.target, &script);
        let failure = result.expect_err("a truncated copy must not be switched to");
        assert_eq!(failure.phase, Phase::Verifying);
        assert!(failure.reason.contains("a.jar"), "{}", failure.reason);
        assert!(!events.iter().any(|e| e == "copy:app.json"), "{events:?}");
        assert!(!r.target.exists());
    }

    #[test]
    fn the_live_launcher_log_may_grow_during_the_move() {
        let r = rig();
        let script = Script {
            after_copy_of: Some(("lucerna.log", AfterCopy::GrowSource)),
            ..Script::default()
        };
        let (result, events) = run(&r.current, &r.target, &script);
        assert_eq!(result, Ok(Outcome::Moved), "{events:?}");
    }

    #[test]
    fn cancel_mid_copy_removes_the_partial_copy() {
        let r = rig();
        let before = listing(&r.current);
        let script = Script {
            cancel_after_files: Some(1),
            ..Script::default()
        };
        let (result, events) = run(&r.current, &r.target, &script);
        assert_eq!(result, Ok(Outcome::Cancelled), "{events:?}");
        assert!(!r.target.exists());
        assert_eq!(listing(&r.current), before);
    }

    #[test]
    fn something_that_started_during_the_copy_aborts_before_app_json() {
        let r = rig();
        let script = Script {
            unsafe_to_switch: true,
            ..Script::default()
        };
        let (result, events) = run(&r.current, &r.target, &script);
        let failure = result.expect_err("the move must not happen");
        assert_eq!(failure.phase, Phase::Verifying);
        assert!(!events.iter().any(|e| e == "copy:app.json"), "{events:?}");
        assert!(!r.target.exists());
    }

    #[test]
    fn an_unreadable_source_directory_aborts_before_the_switch() {
        let r = rig();
        let script = Script {
            fail_list_of: Some("instances"),
            ..Script::default()
        };
        let (result, events) = run(&r.current, &r.target, &script);
        let failure = result.expect_err("an unreadable directory is not 'empty'");
        assert_eq!(failure.phase, Phase::Copying);
        assert!(failure.reason.contains("instances"), "{}", failure.reason);
        assert!(!events.iter().any(|e| e == "commit"), "{events:?}");
    }

    #[test]
    fn app_json_is_deferred_whatever_order_the_directory_lists() {
        let r = rig();
        let script = Script {
            app_json_listed_first: true,
            ..Script::default()
        };
        let (result, events) = run(&r.current, &r.target, &script);
        assert_eq!(result, Ok(Outcome::Moved), "{events:?}");
        let copies: Vec<&String> = events.iter().filter(|e| e.starts_with("copy:")).collect();
        assert_eq!(copies.last().map(|s| s.as_str()), Some("copy:app.json"));
        assert!(position(&events, "phase:Verifying") < position(&events, "copy:app.json"));
    }

    #[test]
    fn a_source_without_app_json_is_refused_up_front() {
        let r = rig();
        std::fs::remove_file(r.current.join("app.json")).unwrap();
        let (result, events) = run(&r.current, &r.target, &Script::default());
        let failure = result.expect_err("there is nothing to switch to");
        assert!(failure.reason.contains("app.json"), "{}", failure.reason);
        assert!(!events.iter().any(|e| e.starts_with("copy:")), "{events:?}");
    }

    #[test]
    fn a_failed_reset_leaves_the_default_dirs_own_entries_in_place() {
        // Reset = custom root → the OS-default dir, which already holds the
        // redirect, the note, its own logs/updates and a stale profile.
        let dir = tempdir().unwrap();
        let current = dir.path().join("custom");
        seed_root(&current);
        let target = dir.path().join("default");
        for (rel, bytes) in [
            ("data-location.json", b"{\"path\":\"x\"}".as_slice()),
            ("pending-cleanup.json", b"{\"webview_dirs\":[]}"),
            ("logs/old.log", b"OLD"),
            ("updates/setup.exe", b"MZ"),
            ("webview/EBWebView/Local State", b"{}"),
        ] {
            let p = target.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }
        let before = listing(&target);

        let script = Script {
            fail_copy_of: Some("level.dat"),
            ..Script::default()
        };
        let (result, events) = run(&current, &target, &script);
        let failure = result.expect_err("the move must not happen");
        assert_eq!(failure.partial_copy_left, None, "{events:?}");
        // The failure is the scripted one from inside the copy — without this
        // the test passes on a pipeline that never touches the target. (No
        // assertion on WHICH files were copied first: directory order is
        // filesystem-specific.)
        assert!(failure.reason.contains("disk full"), "{}", failure.reason);
        let after = listing(&target);
        for kept in &before {
            assert!(after.contains(kept), "{kept} was removed: {after:?}");
        }
        assert!(
            !after.iter().any(|p| p.starts_with("instances/")),
            "{after:?}"
        );
        assert!(!after.iter().any(|p| p == "account.json"), "{after:?}");
    }

    #[test]
    fn retry_removes_what_it_now_can_and_reports_the_rest() {
        let r = rig();
        let first = Script {
            fail_remove_of: Some("libraries"),
            ..Script::default()
        };
        let (result, _) = run(&r.current, &r.target, &first);
        assert!(matches!(result, Ok(Outcome::MovedWithLeftovers { .. })));

        // Still locked: still reported.
        let locked = |p: &Path| -> std::io::Result<()> {
            if name_of(p) == "libraries" {
                Err(scripted_error("scripted: in use"))
            } else {
                real_remove_entry(p)
            }
        };
        let io = Io {
            remove_entry: &locked,
            ..Io::real()
        };
        let names = vec!["libraries".to_string()];
        assert_eq!(retry_leftovers(&r.current, &r.target, &names, &io), names);

        // Released: gone, and an entry that vanished meanwhile is not an error.
        let names = vec!["libraries".to_string(), "already-gone".to_string()];
        assert!(retry_leftovers(&r.current, &r.target, &names, &Io::real()).is_empty());
        assert!(!r.current.join("libraries").exists());
    }

    #[test]
    fn retry_never_removes_a_launcher_owned_entry() {
        let r = rig();
        let (result, _) = run(&r.current, &r.target, &Script::default());
        assert_eq!(result, Ok(Outcome::Moved));
        let names = vec!["webview".to_string()];
        assert_eq!(
            retry_leftovers(&r.current, &r.target, &names, &Io::real()),
            Vec::<String>::new(),
            "a skipped name is simply not this function's business"
        );
        assert!(r.current.join("webview").exists());
    }
}

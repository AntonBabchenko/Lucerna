//! Per-instance journal — an append-only local record of what the launcher
//! did to an instance: content changes (mods, assets, modpack, integrity
//! repair) and finished launch attempts with their outcome.
//!
//! Two jobs. It is a transparency surface (the user can see exactly what was
//! changed and when), and it is evidence for crash diagnosis — a content
//! change recorded minutes before a crash log is the answer to "what did I
//! break?".
//!
//! # Storage
//!
//! `<instance>/.lucerna/journal.jsonl`, one JSON object per line, oldest
//! first. Deliberately NOT under `<instance>/logs/`: that directory is an
//! enumerated log root, so a journal there would show up in the log file
//! list AND be deleted by `logs::retention` (which only protects
//! `latest.log`, `debug.log` and the launcher's own log). Living beside
//! `playtime.json` in `.lucerna/` keeps it out of reach of every log path
//! guard and every cleanup pass.
//!
//! # Failure policy
//!
//! Journaling must never break the operation it records, so call sites use
//! the infallible [`record`]. Read failures degrade to "no history".

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const META_DIR: &str = ".lucerna";
const JOURNAL_FILE: &str = "journal.jsonl";
const TMP_FILE: &str = "journal.jsonl.tmp";

/// Rewrite the file once it grows past this. Chosen so the steady-state file
/// stays small enough to read whole without thought (~a few hundred entries).
const TRIM_TRIGGER_BYTES: u64 = 256 * 1024;
/// Entries kept by a trim. Also the hard ceiling a read can return.
pub const MAX_ENTRIES: usize = 400;
/// Entries returned when the caller passes no explicit limit.
pub const DEFAULT_READ_LIMIT: usize = 200;

/// One recorded moment in an instance's life.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct JournalEntry {
    /// Wall-clock time of the recorded action. `f64` because
    /// specta-typescript forbids 64-bit integer exports.
    pub at_unix_ms: f64,
    pub event: JournalEvent,
}

/// What happened. Two variants, each internally uniform, so the UI renders
/// from a fixed field set per branch instead of a wide grab-bag struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalEvent {
    /// A change to the instance's installed content.
    Content {
        action: ContentAction,
        /// Display name of the affected content (mod title, pack name, asset
        /// filename). Empty for instance-wide actions like integrity repair
        /// or a bulk dependency install.
        subject: String,
        from_version: Option<String>,
        to_version: Option<String>,
        /// Item count for actions that touch more than one file (install with
        /// dependencies, modpack update, integrity repair). `None` for
        /// single-subject actions.
        affected: Option<f64>,
        /// Id of the `.lucerna/reports/<taskId>.json` this row can deep-link
        /// to, so History can reopen the task's per-file detail. `None` for
        /// actions that never produced a report (most single-file changes)
        /// and for every row recorded before install reports existed.
        ///
        /// Additive — rows written before install reports existed deserialise
        /// to None. `skip_serializing_if` keeps old rows byte-identical on
        /// rewrite.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        report_id: Option<String>,
    },
    /// A launch attempt that has finished. In-flight launches are never
    /// recorded — the running-instances popover already shows those, and a
    /// half-written row would be a second source of truth.
    Launch {
        outcome: LaunchOutcome,
        /// `None` when the code is genuinely unknown — the app-exit teardown
        /// kills the game and removes the registry entry before the
        /// exit-watcher can read a status. Recording a fake `0` there would
        /// claim a clean exit that was never observed.
        exit_code: Option<i32>,
        duration_seconds: f64,
        /// The captured game-console log for this run, so a journal row can
        /// deep-link into the log viewer.
        log_path: Option<String>,
    },
}

/// The content-change vocabulary. Adding a variant costs one entry here, one
/// icon/tone/copy row on the frontend, and two locale keys — the frontend
/// completeness test enforces the latter two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ContentAction {
    ModInstalled,
    ModUpdated,
    ModRemoved,
    ModEnabled,
    ModDisabled,
    AssetInstalled,
    AssetUpdated,
    AssetRemoved,
    ModpackImported,
    ModpackUpdated,
    IntegrityRepaired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LaunchOutcome {
    /// Exited cleanly (code 0).
    Ok,
    /// Non-zero exit, or signal termination (`-1`), without a Stop request.
    Crashed,
    /// The user pressed Stop.
    Stopped,
}

/// Classify a finished process. Mirrors `ProcessExited`'s semantics exactly so
/// the journal can never disagree with the exit toast the user just saw:
/// a Stop request wins over the code, because a killed process reports a
/// crash-shaped exit on every platform.
pub fn launch_outcome(user_requested: bool, exit_code: i32) -> LaunchOutcome {
    if user_requested {
        LaunchOutcome::Stopped
    } else if exit_code == 0 {
        LaunchOutcome::Ok
    } else {
        LaunchOutcome::Crashed
    }
}

/// Convenience constructor for a single-subject content change.
pub fn content(action: ContentAction, subject: impl Into<String>) -> JournalEvent {
    JournalEvent::Content {
        action,
        subject: subject.into(),
        from_version: None,
        to_version: None,
        affected: None,
        report_id: None,
    }
}

/// Convenience constructor for a versioned content change. Pass `None` for a
/// side that does not apply (installs have no `from`, removals no `to`).
pub fn content_versioned(
    action: ContentAction,
    subject: impl Into<String>,
    from_version: Option<String>,
    to_version: Option<String>,
) -> JournalEvent {
    JournalEvent::Content {
        action,
        subject: subject.into(),
        from_version,
        to_version,
        affected: None,
        report_id: None,
    }
}

/// Convenience constructor for a bulk content change (`affected` set).
pub fn content_bulk(
    action: ContentAction,
    subject: impl Into<String>,
    affected: usize,
) -> JournalEvent {
    JournalEvent::Content {
        action,
        subject: subject.into(),
        from_version: None,
        to_version: None,
        affected: Some(affected as f64),
        report_id: None,
    }
}

impl JournalEvent {
    /// Attach a persisted report id to a content-change event, so its journal
    /// row can deep-link to `.lucerna/reports/<taskId>.json`. Chains onto the
    /// three constructors above (`content(..).with_report_id(id)`) rather than
    /// widening their signatures, since most call sites never have a report to
    /// attach. No-op on `Launch` — only content-changing tasks produce reports.
    pub fn with_report_id(mut self, report_id: impl Into<String>) -> Self {
        if let JournalEvent::Content {
            report_id: slot, ..
        } = &mut self
        {
            *slot = Some(report_id.into());
        }
        self
    }
}

pub fn journal_path(instance_root: &Path) -> PathBuf {
    instance_root.join(META_DIR).join(JOURNAL_FILE)
}

fn tmp_path(instance_root: &Path) -> PathBuf {
    instance_root.join(META_DIR).join(TMP_FILE)
}

/// Serialises appends and trims within the process. A bulk install fans out
/// several journal writes from concurrent tasks; without this a line could
/// interleave with a trim's rewrite. Cross-process races are excluded by the
/// single-instance guard.
fn write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Append `event` to the instance's journal, stamped with the current time.
///
/// Infallible by design: a journal write is never worth failing the install /
/// launch / repair that produced it. Errors go to the launcher log.
pub fn record(instance_root: &Path, event: JournalEvent) {
    // Timestamp NOW, not when the write lands: most call sites are async and
    // hand the file work to the blocking pool below, so stamping later would let
    // two records taken in a known order come out reordered.
    let entry = JournalEntry {
        at_unix_ms: chrono::Utc::now().timestamp_millis() as f64,
        event,
    };
    // Off the async worker when there is a runtime to offload onto. The write is
    // small, but it is still an open + append + occasional 256 KB rewrite under a
    // blocking mutex — that has no business sitting on a tokio worker thread
    // where it would stall unrelated tasks (AV scanners on Windows routinely add
    // tens of milliseconds per write).
    //
    // No runtime means a synchronous caller — notably `kill_all_running` on app
    // exit, where writing inline is not just acceptable but REQUIRED: a spawned
    // task would not survive process teardown, and that launch row would vanish.
    match tokio::runtime::Handle::try_current() {
        Ok(_) => {
            let root = instance_root.to_path_buf();
            tokio::task::spawn_blocking(move || write_or_log(&root, &entry));
        }
        Err(_) => write_or_log(instance_root, &entry),
    }
}

fn write_or_log(instance_root: &Path, entry: &JournalEntry) {
    if let Err(e) = append_entry(instance_root, entry) {
        crate::diag!(
            "journal: failed to record at {}: {e}",
            instance_root.display()
        );
    }
}

/// Fallible, synchronous append of `event` stamped with the current time.
/// Used by tests, which need the error and a deterministic completion.
#[cfg(test)]
fn append(instance_root: &Path, event: JournalEvent) -> Result<()> {
    let entry = JournalEntry {
        at_unix_ms: chrono::Utc::now().timestamp_millis() as f64,
        event,
    };
    append_entry(instance_root, &entry)
}

/// Append a fully-formed entry (timestamp included). Used by [`append`] and by
/// tests that need deterministic timestamps.
fn append_entry(instance_root: &Path, entry: &JournalEntry) -> Result<()> {
    use std::io::Write;

    let path = journal_path(instance_root);
    let mut line = serde_json::to_string(entry)
        .map_err(|e| Error::io(path.display().to_string(), format!("serialize: {e}")))?;
    line.push('\n');

    let _guard = write_lock().lock().unwrap_or_else(|p| p.into_inner());

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display().to_string(), e))?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| Error::io(path.display().to_string(), e))?;
    f.write_all(line.as_bytes())
        .map_err(|e| Error::io(path.display().to_string(), e))?;
    drop(f);

    trim_if_needed(instance_root, &path);
    Ok(())
}

/// Rewrite the journal down to the newest [`MAX_ENTRIES`] once it grows past
/// [`TRIM_TRIGGER_BYTES`]. Best-effort: a failed trim leaves an oversized but
/// perfectly readable file, so it must not surface as an append error.
fn trim_if_needed(instance_root: &Path, path: &Path) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() <= TRIM_TRIGGER_BYTES {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let all = parse_lines(&text);
    if all.len() <= MAX_ENTRIES {
        // Oversized but few entries (pathologically long subjects). Dropping
        // rows here would lose history for no gain — leave it.
        return;
    }
    let keep = &all[all.len() - MAX_ENTRIES..];
    let mut out = String::new();
    for e in keep {
        match serde_json::to_string(e) {
            Ok(s) => {
                out.push_str(&s);
                out.push('\n');
            }
            Err(_) => return, // re-serialisation cannot fail for parsed rows; bail rather than truncate
        }
    }
    let tmp = tmp_path(instance_root);
    if std::fs::write(&tmp, out.as_bytes()).is_err() {
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        crate::diag!("journal: trim rename failed for {}: {e}", path.display());
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Parse a journal body into entries, oldest first. Unparseable lines are
/// skipped: a torn last line from an interrupted write, or a row written by a
/// newer launcher with an event kind this build does not know, must not hide
/// the rest of the history.
fn parse_lines(text: &str) -> Vec<JournalEntry> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<JournalEntry>(l).ok())
        .collect()
}

/// Read the newest `limit` entries, newest first. A missing file is an empty
/// history, not an error — a fresh instance simply has none.
pub fn read(instance_root: &Path, limit: usize) -> Result<Vec<JournalEntry>> {
    let path = journal_path(instance_root);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::io(path.display().to_string(), e)),
    };
    let mut all = parse_lines(&text);
    let take = limit.clamp(1, MAX_ENTRIES);
    if all.len() > take {
        all.drain(..all.len() - take);
    }
    all.reverse();
    Ok(all)
}

/// Delete the journal. Missing file is a no-op — "clear" is idempotent.
///
/// Takes the same lock as append/trim. Without it, a clear racing an in-flight
/// append can unlink the file from under a still-open handle (both POSIX unlink
/// and Windows `FILE_SHARE_DELETE` allow this), and the append's already-reported
/// line is silently discarded when that handle closes.
pub fn clear(instance_root: &Path) -> Result<()> {
    let _guard = write_lock().lock().unwrap_or_else(|p| p.into_inner());
    let path = journal_path(instance_root);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(path.display().to_string(), e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_at(ms: f64, subject: &str) -> JournalEntry {
        JournalEntry {
            at_unix_ms: ms,
            event: content(ContentAction::ModInstalled, subject),
        }
    }

    #[test]
    fn missing_file_reads_as_empty_history() {
        let td = tempfile::tempdir().unwrap();
        let got = read(td.path(), 50).expect("missing journal is not an error");
        assert!(got.is_empty());
    }

    #[test]
    fn round_trip_returns_newest_first() {
        let td = tempfile::tempdir().unwrap();
        append_entry(td.path(), &entry_at(1_000.0, "First")).unwrap();
        append_entry(td.path(), &entry_at(2_000.0, "Second")).unwrap();

        let got = read(td.path(), 50).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].at_unix_ms, 2_000.0);
        assert_eq!(got[1].at_unix_ms, 1_000.0);
        match &got[0].event {
            JournalEvent::Content { subject, .. } => assert_eq!(subject, "Second"),
            other => panic!("expected Content, got {other:?}"),
        }
    }

    #[test]
    fn journal_lives_outside_every_per_instance_log_root() {
        // Storage-location guarantee. `logs::files::allowed_roots` enumerates
        // these three per-instance directories, and everything inside them is
        // fair game for `logs::retention` (which protects only `latest.log`,
        // `debug.log`, and the app-wide launcher log). A journal in any of them
        // would be listed as a log file AND silently deleted by a retention
        // pass. Mirrors the root shapes rather than calling `allowed_roots`,
        // which needs an AppHandle.
        let td = tempfile::tempdir().unwrap();
        let inst = td.path();
        let log_roots = [
            inst.join(".minecraft").join("logs"),
            inst.join(".minecraft").join("crash-reports"),
            inst.join("logs"),
        ];
        let p = journal_path(inst);
        for root in &log_roots {
            assert!(
                !p.starts_with(root),
                "journal must not live under log root {}: {}",
                root.display(),
                p.display()
            );
        }
        assert_eq!(
            p.parent().and_then(|d| d.file_name()),
            Some(META_DIR.as_ref())
        );
    }

    #[test]
    fn malformed_line_is_skipped_and_neighbours_survive() {
        let td = tempfile::tempdir().unwrap();
        append_entry(td.path(), &entry_at(1_000.0, "Good one")).unwrap();
        // A torn write: half a line with no trailing newline resolution.
        let path = journal_path(td.path());
        let mut body = std::fs::read_to_string(&path).unwrap();
        body.push_str("{\"at_unix_ms\": 1500, \"eve\n");
        std::fs::write(&path, &body).unwrap();
        append_entry(td.path(), &entry_at(2_000.0, "Good two")).unwrap();

        let got = read(td.path(), 50).unwrap();
        assert_eq!(got.len(), 2, "the torn line is dropped, the rest survives");
        assert_eq!(got[0].at_unix_ms, 2_000.0);
        assert_eq!(got[1].at_unix_ms, 1_000.0);
    }

    #[test]
    fn unknown_event_kind_is_skipped() {
        // Forward compatibility: a newer launcher may write kinds this build
        // has never heard of. Those rows drop out; everything else renders.
        let td = tempfile::tempdir().unwrap();
        append_entry(td.path(), &entry_at(1_000.0, "Known")).unwrap();
        let path = journal_path(td.path());
        let mut body = std::fs::read_to_string(&path).unwrap();
        body.push_str(
            "{\"at_unix_ms\":1500.0,\"event\":{\"kind\":\"teleportation\",\"whither\":\"nether\"}}\n",
        );
        std::fs::write(&path, &body).unwrap();

        let got = read(td.path(), 50).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].at_unix_ms, 1_000.0);
    }

    #[test]
    fn read_limit_returns_the_newest_slice() {
        let td = tempfile::tempdir().unwrap();
        for i in 0..10 {
            append_entry(td.path(), &entry_at(i as f64 * 1_000.0, &format!("m{i}"))).unwrap();
        }
        let got = read(td.path(), 3).unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].at_unix_ms, 9_000.0);
        assert_eq!(got[2].at_unix_ms, 7_000.0);
    }

    #[test]
    fn read_clamps_the_limit_to_one_at_the_bottom_and_max_entries_at_the_top() {
        // `read` clamps to `[1, MAX_ENTRIES]`; substituting a sensible page size
        // for `0` is the COMMAND layer's job, not this function's. A `0` that
        // silently returned an empty history would look like "no activity".
        let td = tempfile::tempdir().unwrap();
        for i in 0..5 {
            append_entry(td.path(), &entry_at(i as f64 * 1_000.0, &format!("m{i}"))).unwrap();
        }
        let floor = read(td.path(), 0).unwrap();
        assert_eq!(floor.len(), 1, "0 clamps up to 1, never to empty");
        assert_eq!(floor[0].at_unix_ms, 4_000.0, "and it is the NEWEST entry");
        // An absurd limit is capped rather than trusted; with only 5 rows on
        // disk that means all 5, not a panic and not MAX_ENTRIES of padding.
        assert_eq!(read(td.path(), usize::MAX).unwrap().len(), 5);
    }

    #[test]
    fn trim_keeps_the_newest_entries_once_over_the_byte_trigger() {
        let td = tempfile::tempdir().unwrap();
        // Long subjects so the byte trigger is reached well before the entry
        // count would need thousands of rows.
        let filler = "x".repeat(600);
        let total = 700;
        for i in 0..total {
            append_entry(td.path(), &entry_at(i as f64, &format!("{filler}-{i:04}"))).unwrap();
        }
        let body = std::fs::read_to_string(journal_path(td.path())).unwrap();
        let kept = parse_lines(&body);
        assert!(
            kept.len() <= MAX_ENTRIES,
            "trim must cap the file at MAX_ENTRIES, got {}",
            kept.len()
        );
        // The survivors are the NEWEST ones: the last append is still present.
        let newest = kept.last().expect("at least one entry survives");
        assert_eq!(newest.at_unix_ms, (total - 1) as f64);
        assert!(
            kept.first().expect("non-empty").at_unix_ms > 0.0,
            "oldest rows were the ones dropped"
        );
    }

    #[test]
    fn record_swallows_a_write_failure() {
        // `.lucerna` occupied by a FILE makes create_dir_all fail. `record`
        // must stay silent: journaling never breaks the caller's operation.
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join(".lucerna"), b"not a dir").unwrap();
        record(td.path(), content(ContentAction::ModRemoved, "Sodium"));
        // And the fallible core does report it, so callers that care can.
        assert!(append(td.path(), content(ContentAction::ModRemoved, "Sodium")).is_err());
    }

    #[test]
    fn concurrent_records_produce_whole_non_interleaved_lines() {
        // The guarantee the write lock exists for. A bulk install fans several
        // journal writes out across threads; without serialisation two partial
        // lines could interleave and BOTH rows would be lost to the
        // skip-malformed reader — silently, since `record` reports nothing.
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        const THREADS: usize = 8;
        const PER_THREAD: usize = 25;
        let handles: Vec<_> = (0..THREADS)
            .map(|t| {
                let root = root.clone();
                std::thread::spawn(move || {
                    for i in 0..PER_THREAD {
                        append(
                            &root,
                            content(ContentAction::ModInstalled, format!("t{t}-{i}")),
                        )
                        .expect("append succeeds");
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("worker thread did not panic");
        }

        let body = std::fs::read_to_string(journal_path(&root)).unwrap();
        let total_lines = body.lines().filter(|l| !l.trim().is_empty()).count();
        let parsed = parse_lines(&body);
        assert_eq!(
            total_lines,
            THREADS * PER_THREAD,
            "every append wrote a line"
        );
        assert_eq!(
            parsed.len(),
            THREADS * PER_THREAD,
            "and every line parsed — none were torn by an interleaved write"
        );
    }

    #[test]
    fn clear_removes_history_and_is_idempotent() {
        let td = tempfile::tempdir().unwrap();
        append_entry(td.path(), &entry_at(1_000.0, "gone")).unwrap();
        clear(td.path()).unwrap();
        assert!(read(td.path(), 50).unwrap().is_empty());
        clear(td.path()).expect("clearing an absent journal is a no-op");
    }

    #[test]
    fn launch_outcome_maps_stop_clean_and_crash() {
        // Stop wins over the exit code: a killed JVM reports a crash-shaped
        // code on every platform, and the user knows they asked for it.
        assert_eq!(launch_outcome(true, 1), LaunchOutcome::Stopped);
        assert_eq!(launch_outcome(true, 0), LaunchOutcome::Stopped);
        assert_eq!(launch_outcome(true, -1), LaunchOutcome::Stopped);
        assert_eq!(launch_outcome(false, 0), LaunchOutcome::Ok);
        assert_eq!(launch_outcome(false, 1), LaunchOutcome::Crashed);
        assert_eq!(launch_outcome(false, -1), LaunchOutcome::Crashed);
    }

    #[test]
    fn launch_entries_round_trip_with_their_outcome() {
        let td = tempfile::tempdir().unwrap();
        append_entry(
            td.path(),
            &JournalEntry {
                at_unix_ms: 5_000.0,
                event: JournalEvent::Launch {
                    outcome: LaunchOutcome::Crashed,
                    exit_code: Some(-1),
                    duration_seconds: 12.0,
                    log_path: Some("C:/i/logs/2026-launch.log".into()),
                },
            },
        )
        .unwrap();
        let got = read(td.path(), 10).unwrap();
        match &got[0].event {
            JournalEvent::Launch {
                outcome,
                exit_code,
                duration_seconds,
                log_path,
            } => {
                assert_eq!(*outcome, LaunchOutcome::Crashed);
                assert_eq!(*exit_code, Some(-1));
                assert_eq!(*duration_seconds, 12.0);
                assert_eq!(log_path.as_deref(), Some("C:/i/logs/2026-launch.log"));
            }
            other => panic!("expected Launch, got {other:?}"),
        }
    }

    #[test]
    fn bulk_and_versioned_constructors_fill_the_right_fields() {
        match content_bulk(ContentAction::ModpackUpdated, "All the Mods 9", 42) {
            JournalEvent::Content {
                affected, subject, ..
            } => {
                assert_eq!(affected, Some(42.0));
                assert_eq!(subject, "All the Mods 9");
            }
            other => panic!("expected Content, got {other:?}"),
        }
        match content_versioned(
            ContentAction::ModUpdated,
            "Sodium",
            Some("0.5.8".into()),
            Some("0.6.0".into()),
        ) {
            JournalEvent::Content {
                from_version,
                to_version,
                affected,
                ..
            } => {
                assert_eq!(from_version.as_deref(), Some("0.5.8"));
                assert_eq!(to_version.as_deref(), Some("0.6.0"));
                assert_eq!(affected, None);
            }
            other => panic!("expected Content, got {other:?}"),
        }
    }

    #[test]
    fn a_content_row_without_report_id_still_parses() {
        // parse_lines silently DROPS any line that fails to deserialize, so a
        // regression here wipes the user's whole visible history with no
        // error surface. Rows written before install reports existed have no
        // `report_id` key at all — they must keep parsing as `None`, not get
        // silently dropped.
        let old = r#"{"at_unix_ms":1000.0,"event":{"kind":"content","action":"mod_installed","subject":"x","from_version":null,"to_version":null,"affected":null}}"#;
        let parsed: JournalEntry = serde_json::from_str(old).unwrap();
        assert!(matches!(
            parsed.event,
            JournalEvent::Content {
                report_id: None,
                ..
            }
        ));
    }

    #[test]
    fn content_row_with_report_id_round_trips() {
        let td = tempfile::tempdir().unwrap();
        append_entry(
            td.path(),
            &JournalEntry {
                at_unix_ms: 1_000.0,
                event: content(ContentAction::ModInstalled, "Sodium").with_report_id("task-abc"),
            },
        )
        .unwrap();

        let got = read(td.path(), 10).unwrap();
        match &got[0].event {
            JournalEvent::Content { report_id, .. } => {
                assert_eq!(report_id.as_deref(), Some("task-abc"));
            }
            other => panic!("expected Content, got {other:?}"),
        }
    }

    #[test]
    fn content_row_without_report_id_omits_the_key_when_serialized() {
        // `skip_serializing_if` must keep old rows byte-identical on rewrite
        // (the journal trims by re-serializing kept entries) — a `None` must
        // vanish from the JSON, not round-trip as an explicit `null`.
        let event = content(ContentAction::ModInstalled, "Sodium");
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            !json.contains("report_id"),
            "a None report_id must be omitted entirely, got: {json}"
        );
    }

    #[test]
    fn with_report_id_is_a_noop_on_launch_events() {
        let event = JournalEvent::Launch {
            outcome: LaunchOutcome::Ok,
            exit_code: Some(0),
            duration_seconds: 1.0,
            log_path: None,
        }
        .with_report_id("task-abc");
        assert!(matches!(event, JournalEvent::Launch { .. }));
    }
}

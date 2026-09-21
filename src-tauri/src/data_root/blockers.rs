//! What stops the launcher from moving its data root or restarting.
//!
//! Replaces `commands::data_location::any_game_running -> bool`, which read
//! "could not tell" as "nothing is running": a `server_list` error mapped to
//! `false`, a server whose `server.json` did not parse was never checked, the
//! `starting` sets were invisible, and a PID whose image could not be queried
//! (always, on macOS) counted as not ours.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RestartBlock {
    None,
    /// A game or server process is live or starting.
    Running,
    /// A long operation holds a claim (maintenance, shared write or read, upload, AI pre-fill).
    Busy,
    /// It could not be checked. Refuses, like the other two.
    Unknown,
}

pub use crate::platform::ImageMatch;

/// What one `servers/<dir>` says about a live server process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PidProbe {
    /// No pid file, or content that cannot name a process.
    NoPid,
    /// A recorded PID that is dead, or alive but provably not a JVM.
    NotOurs,
    /// A live JVM recorded by this server: it is running.
    Ours,
    /// The pid file could not be read, or the PID is alive and its image unknown.
    Unknown,
}

/// Everything `classify` needs, observed by the caller.
pub struct Observed {
    pub client_running_or_starting: bool,
    pub server_running_or_starting: bool,
    /// One probe per directory under `servers/`; `Err` = `servers/` itself
    /// could not be enumerated (absent is `Ok(vec![])`).
    pub server_dirs: Result<Vec<PidProbe>, ()>,
    pub claim_held: bool,
}

/// Pure decision. Precedence: Running > Busy > Unknown > None — the most
/// actionable reason wins; all three refuse.
pub fn classify(observed: &Observed) -> RestartBlock {
    let dirs = observed.server_dirs.as_deref();
    let any_dir_is = |wanted: PidProbe| dirs.is_ok_and(|probes| probes.contains(&wanted));
    if observed.client_running_or_starting
        || observed.server_running_or_starting
        || any_dir_is(PidProbe::Ours)
    {
        RestartBlock::Running
    } else if observed.claim_held {
        RestartBlock::Busy
    } else if dirs.is_err() || any_dir_is(PidProbe::Unknown) {
        RestartBlock::Unknown
    } else {
        RestartBlock::None
    }
}

/// Probe `<server_dir>/runtime/server.pid` without touching `server.json`.
pub fn probe_server_dir(
    server_dir: &Path,
    alive: &dyn Fn(u32) -> bool,
    image: &dyn Fn(u32) -> ImageMatch,
) -> PidProbe {
    let pid_file = server_dir.join("runtime").join("server.pid");
    let raw = match std::fs::read_to_string(&pid_file) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return PidProbe::NoPid,
        // Unreadable is not absent.
        Err(_) => return PidProbe::Unknown,
    };
    // Garbage cannot name a process, so there is nothing to find alive.
    let Ok(pid) = raw.trim().parse::<u32>() else {
        return PidProbe::NoPid;
    };
    if !alive(pid) {
        return PidProbe::NotOurs;
    }
    match image(pid) {
        ImageMatch::Yes => PidProbe::Ours,
        ImageMatch::No => PidProbe::NotOurs,
        ImageMatch::Unknown => PidProbe::Unknown,
    }
}

/// One probe per directory under `servers_root`. Non-directories (`.DS_Store`)
/// are ignored; an entry that cannot be inspected is `Unknown`.
pub(crate) fn probe_all(servers_root: &Path) -> Result<Vec<PidProbe>, ()> {
    let children = match crate::data_root::walk::real_list_dir(servers_root) {
        Ok(children) => children,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            crate::diag!(
                "[blockers] cannot enumerate {}: {e}",
                servers_root.display()
            );
            return Err(());
        }
    };
    Ok(children
        .iter()
        .filter_map(|child| match std::fs::symlink_metadata(child) {
            Ok(meta) if meta.is_dir() => Some(probe_server_dir(
                child,
                &crate::platform::process_alive,
                &|pid| crate::platform::process_image_probe(pid, "java"),
            )),
            Ok(_) => None,
            Err(_) => Some(PidProbe::Unknown),
        })
        .collect())
}

/// What, if anything, stops a data-root change or a restart right now.
pub fn observe(app: &tauri::AppHandle) -> RestartBlock {
    let server_dirs = match crate::paths::servers_dir(app) {
        Ok(dir) => probe_all(&dir),
        Err(e) => {
            crate::diag!("[blockers] cannot resolve the servers dir: {e}");
            Err(())
        }
    };
    classify(&Observed {
        client_running_or_starting: crate::launch::spawn::is_any_running()
            || crate::launch::spawn::is_any_starting(),
        server_running_or_starting: !crate::servers_runtime::runtime::running_ids_snapshot()
            .is_empty()
            || crate::servers_runtime::runtime::is_any_starting(),
        server_dirs,
        claim_held: crate::instances::maintenance::any_active()
            || crate::servers_runtime::maintenance::any_active()
            || crate::servers_runtime::upload_control::upload_any_active()
            || crate::l10n::prefill::cancel::any_active(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn quiet() -> Observed {
        Observed {
            client_running_or_starting: false,
            server_running_or_starting: false,
            server_dirs: Ok(vec![PidProbe::NoPid, PidProbe::NotOurs]),
            claim_held: false,
        }
    }

    #[test]
    fn nothing_observed_means_none() {
        assert_eq!(classify(&quiet()), RestartBlock::None);
    }

    #[test]
    fn anything_live_or_starting_is_running() {
        for o in [
            Observed {
                client_running_or_starting: true,
                ..quiet()
            },
            Observed {
                server_running_or_starting: true,
                ..quiet()
            },
            Observed {
                server_dirs: Ok(vec![PidProbe::NoPid, PidProbe::Ours]),
                ..quiet()
            },
        ] {
            assert_eq!(classify(&o), RestartBlock::Running);
        }
    }

    #[test]
    fn a_held_claim_is_busy() {
        let o = Observed {
            claim_held: true,
            ..quiet()
        };
        assert_eq!(classify(&o), RestartBlock::Busy);
    }

    #[test]
    fn could_not_tell_is_unknown_never_none() {
        for o in [
            Observed {
                server_dirs: Err(()),
                ..quiet()
            },
            Observed {
                server_dirs: Ok(vec![PidProbe::Unknown]),
                ..quiet()
            },
        ] {
            assert_eq!(classify(&o), RestartBlock::Unknown);
        }
    }

    #[test]
    fn the_most_actionable_reason_wins() {
        let everything = Observed {
            client_running_or_starting: true,
            server_running_or_starting: false,
            server_dirs: Err(()),
            claim_held: true,
        };
        assert_eq!(classify(&everything), RestartBlock::Running);
        let busy_and_unknown = Observed {
            client_running_or_starting: false,
            ..everything
        };
        assert_eq!(classify(&busy_and_unknown), RestartBlock::Busy);
    }

    fn server_dir_with_pid(content: Option<&[u8]>) -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempdir().unwrap();
        let dir = d.path().join("my-server");
        std::fs::create_dir_all(dir.join("runtime")).unwrap();
        // A server.json that does NOT parse: the probe must not care.
        std::fs::write(dir.join("server.json"), b"{ not json").unwrap();
        if let Some(bytes) = content {
            std::fs::write(dir.join("runtime").join("server.pid"), bytes).unwrap();
        }
        (d, dir)
    }

    #[test]
    fn a_live_jvm_behind_an_unparseable_server_json_is_still_found() {
        let (_d, dir) = server_dir_with_pid(Some(b"4242\n"));
        let probe = probe_server_dir(&dir, &|pid| pid == 4242, &|_| ImageMatch::Yes);
        assert_eq!(probe, PidProbe::Ours);
    }

    #[test]
    fn dead_or_foreign_pids_are_not_ours() {
        let (_d, dir) = server_dir_with_pid(Some(b"4242"));
        assert_eq!(
            probe_server_dir(&dir, &|_| false, &|_| ImageMatch::Yes),
            PidProbe::NotOurs
        );
        assert_eq!(
            probe_server_dir(&dir, &|_| true, &|_| ImageMatch::No),
            PidProbe::NotOurs
        );
    }

    #[test]
    fn a_live_pid_with_an_unknown_image_is_unknown_not_absent() {
        let (_d, dir) = server_dir_with_pid(Some(b"4242"));
        assert_eq!(
            probe_server_dir(&dir, &|_| true, &|_| ImageMatch::Unknown),
            PidProbe::Unknown
        );
    }

    #[test]
    fn a_missing_or_garbage_pid_file_names_no_process() {
        let (_d, dir) = server_dir_with_pid(None);
        assert_eq!(
            probe_server_dir(&dir, &|_| true, &|_| ImageMatch::Yes),
            PidProbe::NoPid
        );
        let (_d2, dir2) = server_dir_with_pid(Some(b"not-a-number"));
        assert_eq!(
            probe_server_dir(&dir2, &|_| true, &|_| ImageMatch::Yes),
            PidProbe::NoPid
        );
    }

    #[test]
    fn an_unreadable_pid_file_is_unknown() {
        // A DIRECTORY where the pid file should be: reading it fails with
        // something other than NotFound on every platform.
        let d = tempdir().unwrap();
        let dir = d.path().join("my-server");
        std::fs::create_dir_all(dir.join("runtime").join("server.pid")).unwrap();
        assert_eq!(
            probe_server_dir(&dir, &|_| true, &|_| ImageMatch::Yes),
            PidProbe::Unknown
        );
    }

    #[test]
    fn restart_block_serializes_as_a_bare_snake_case_token() {
        assert_eq!(
            serde_json::to_string(&RestartBlock::None).unwrap(),
            r#""none""#
        );
        assert_eq!(
            serde_json::to_string(&RestartBlock::Unknown).unwrap(),
            r#""unknown""#
        );
    }
    #[test]
    fn probe_all_ignores_files_and_treats_a_missing_root_as_no_servers() {
        let d = tempdir().unwrap();
        assert_eq!(probe_all(&d.path().join("servers")), Ok(Vec::new()));

        let root = d.path().join("servers");
        std::fs::create_dir_all(root.join("alpha").join("runtime")).unwrap();
        std::fs::write(root.join(".DS_Store"), b"x").unwrap();
        assert_eq!(probe_all(&root), Ok(vec![PidProbe::NoPid]));
    }
}

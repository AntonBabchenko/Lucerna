//! Settings sync (spec 2026-10-15 Part B): an instance can share its server
//! list, command history and hotbars with every other instance that opts in.
//!
//! One shared copy per kind lives in `<data_root>/shared/settings/`; each
//! member instance keeps a sidecar (`lucerna/settings-sync.json`) and, per
//! kind, a baseline of what its file held when Lucerna last touched it. A
//! *sync point* (before a launch, after the game exits, on join) captures what
//! changed in this instance since that baseline into the shared copy and
//! applies the shared copy back. Everything here takes `&Path` roots: no
//! `AppHandle`, so it is tested directly.

pub(crate) mod baseline;
pub mod creation;
pub mod engine;
pub mod failure;
pub(crate) mod fsio;
pub(crate) mod history;
pub mod hotbars;
pub mod marker;
pub mod membership;
pub(crate) mod options;
pub(crate) mod servers;
pub mod sidecar;
pub mod store;
pub mod version;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One thing that syncs. Part C adds `Options`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Servers,
    History,
    Hotbars,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Servers, Kind::History, Kind::Hotbars];

    /// The file the game keeps for this kind, in `.minecraft/` and in the
    /// shared store alike.
    pub fn file(self) -> &'static str {
        match self {
            Kind::Servers => "servers.dat",
            Kind::History => "command_history.txt",
            Kind::Hotbars => "hotbar.nbt",
        }
    }

    fn stem(self) -> &'static str {
        match self {
            Kind::Servers => "servers",
            Kind::History => "history",
            Kind::Hotbars => "hotbars",
        }
    }
}

/// `<instance>/lucerna/settings-sync.json`.
pub fn sidecar_path(instance_root: &Path) -> PathBuf {
    instance_root.join("lucerna").join("settings-sync.json")
}

/// `<instance>/lucerna/settings-sync/`: baselines, pending files, the marker.
pub fn state_dir(instance_root: &Path) -> PathBuf {
    instance_root.join("lucerna").join("settings-sync")
}

/// `<instance>/.minecraft/<file>`.
pub fn game_file(instance_root: &Path, kind: Kind) -> PathBuf {
    instance_root.join(".minecraft").join(kind.file())
}

pub use sidecar::SyncFailure;

/// Before a launch (spec §B3): every kind this instance syncs, in order. It
/// stops at the first failure; the kinds before it stay synced, and the
/// failing kind's file was not written (its last step is the atomic write).
pub fn before_launch(
    data_root: &Path,
    instance_root: &Path,
    facts: &version::VersionFacts,
) -> std::result::Result<(), SyncFailure> {
    run_all(data_root, instance_root, facts, false)
}

/// After the game exits: the same, and a failure is kept in the sidecar for
/// the section to show until a later sync succeeds.
pub fn after_exit(
    data_root: &Path,
    instance_root: &Path,
    facts: &version::VersionFacts,
) -> std::result::Result<(), SyncFailure> {
    run_all(data_root, instance_root, facts, true)
}

/// After Lucerna itself edited this instance's `kind` file (the saved-servers
/// manager): the same sync point, for that kind alone, so the other instances
/// see the edit without a launch. A failure is kept for the section, as after
/// an exit: nobody is waiting on a dialog for it. `facts` is read only when
/// this instance syncs `kind`.
///
/// Not while a game Lucerna lost track of may still run here: it rewrites the
/// file when it closes, and the next sync point would read what this one
/// applied as removals. The edit stays local, the baseline does not move, and
/// the next launch asks about that game first.
pub fn after_local_edit(
    data_root: &Path,
    instance_root: &Path,
    kind: Kind,
    probe: &dyn Fn(u32) -> crate::platform::ImageMatch,
    facts: impl FnOnce() -> version::VersionFacts,
) -> std::result::Result<(), SyncFailure> {
    match sidecar::read(instance_root) {
        Ok(sidecar::SidecarRead::Member(s)) if s.enabled(kind) => {}
        Ok(_) => return Ok(()),
        Err(_) => {
            // An unreadable sidecar may well sync this kind: `run_kinds`
            // reads it again and names it in the failure.
        }
    }
    match marker::check(instance_root, probe) {
        Ok(marker::MarkerState::Clear) => {}
        Ok(state) => {
            crate::diag!(
                "settings sync: {} not synced after an edit: a game started before Lucerna \
                 restarted may still run ({state:?})",
                kind.file()
            );
            return Ok(());
        }
        Err(e) => return Err(SyncFailure::logged(Some(kind), &e)),
    }
    run_kinds(data_root, instance_root, &facts(), &[kind], true)
}

/// The game files this instance syncs, relative to `.minecraft/`: what a pack
/// files re-import must leave alone (spec §B8). An unreadable sidecar is an
/// error, not "none": overwriting a file that may be shared would spread the
/// pack's copy, and skipping it would claim it is shared.
pub fn synced_files(instance_root: &Path) -> crate::error::Result<Vec<&'static str>> {
    Ok(match sidecar::read(instance_root)? {
        sidecar::SidecarRead::NotMember => vec![],
        sidecar::SidecarRead::Member(s) => Kind::ALL
            .into_iter()
            .filter(|k| s.enabled(*k))
            .map(Kind::file)
            .collect(),
    })
}

/// Everything a launch does before spawning (blocking; call it inside
/// `spawn_blocking`): nothing for an instance that syncs nothing; otherwise
/// the running-game marker is checked, the version facts are read and every
/// kind syncs. `Some(facts)` means the instance syncs: the caller writes the
/// marker after spawning and hands the facts to the exit step.
pub fn launch_sync(
    data_root: &Path,
    instance_root: &Path,
    versions_dir: &Path,
    mc_version: &str,
    game_closed_confirmed: bool,
    probe: &dyn Fn(u32) -> crate::platform::ImageMatch,
) -> crate::error::Result<Option<version::VersionFacts>> {
    use crate::error::Error;
    let syncs = match sidecar::read(instance_root) {
        Ok(sidecar::SidecarRead::NotMember) => false,
        Ok(sidecar::SidecarRead::Member(s)) => Kind::ALL.iter().any(|k| s.enabled(*k)),
        // An unreadable sidecar may well sync: `before_launch` names it below.
        Err(_) => true,
    };
    if !syncs {
        return Ok(None);
    }
    guard_orphan(instance_root, game_closed_confirmed, probe)?;
    let facts = version::VersionFacts::read(versions_dir, mc_version);
    before_launch(data_root, instance_root, &facts).map_err(Error::from)?;
    Ok(Some(facts))
}

/// Refuse to write this instance's synced files while a game Lucerna lost
/// track of (one started before a launcher restart) may still run in it
/// (spec §B3): it rewrites them when it closes, and the next sync point would
/// read what it dropped as removals and take them out of the shared copy.
/// A live game refuses; "could not tell" asks, and the user's "It's closed"
/// removes the marker.
pub fn guard_orphan(
    instance_root: &Path,
    game_closed_confirmed: bool,
    probe: &dyn Fn(u32) -> crate::platform::ImageMatch,
) -> crate::error::Result<()> {
    use crate::error::Error;
    match marker::check(instance_root, probe)? {
        marker::MarkerState::Clear => Ok(()),
        marker::MarkerState::Alive => Err(Error::SettingsSyncGameRunning),
        marker::MarkerState::Unknown if game_closed_confirmed => marker::remove(instance_root),
        marker::MarkerState::Unknown => Err(Error::SettingsSyncAskGameClosed),
    }
}

fn run_all(
    data_root: &Path,
    instance_root: &Path,
    facts: &version::VersionFacts,
    keep_failure: bool,
) -> std::result::Result<(), SyncFailure> {
    run_kinds(data_root, instance_root, facts, &Kind::ALL, keep_failure)
}

/// The sync point for `kinds` (those this instance has on), in order. A kept
/// failure is cleared by a success only when it was about one of `kinds` (or
/// the sidecar itself, which was just read): syncing the server list says
/// nothing about the hotbars.
fn run_kinds(
    data_root: &Path,
    instance_root: &Path,
    facts: &version::VersionFacts,
    kinds: &[Kind],
    keep_failure: bool,
) -> std::result::Result<(), SyncFailure> {
    let before = match sidecar::read(instance_root) {
        Ok(sidecar::SidecarRead::NotMember) => return Ok(()),
        Ok(sidecar::SidecarRead::Member(s)) => s,
        Err(e) => return Err(SyncFailure::logged(None, &e)),
    };
    let store = store::lock(data_root);
    let mut after = before.clone();
    let mut failed = None;
    for &kind in kinds {
        let Some(state) = before.kind(kind).filter(|s| s.enabled) else {
            continue;
        };
        match engine::sync_kind(
            instance_root,
            &store,
            facts,
            kind,
            state.join,
            &engine::Seams::none(),
        ) {
            Ok(engine::KindOutcome::Joined | engine::KindOutcome::Synced) => {
                after.set_kind(
                    kind,
                    sidecar::KindState {
                        enabled: true,
                        join: sidecar::Join::Done,
                    },
                );
            }
            // Not applied (an unsupported version, older than the shared
            // hotbars): a pending join waits for a sync point that can.
            Ok(_) => {}
            Err(e) => {
                failed = Some(SyncFailure::logged(Some(kind), &e));
                break;
            }
        }
    }
    after.last_error = match &failed {
        Some(f) if keep_failure => Some(f.clone()),
        Some(_) => before.last_error.clone(),
        None => before
            .last_error
            .clone()
            .filter(|kept| kept.kind.is_some_and(|k| !kinds.contains(&k))),
    };
    if after != before {
        if let Err(e) = sidecar::write(instance_root, &after) {
            let f = SyncFailure::logged(None, &e);
            return Err(failed.unwrap_or(f));
        }
    }
    match failed {
        Some(f) => Err(f),
        None => Ok(()),
    }
}

#[cfg(test)]
mod api_tests {
    use super::failure::{FileReason, SyncFile};
    use super::sidecar::{Join, KindState, Sidecar, SidecarRead};
    use super::version::{DataVersion, VersionFacts};
    use super::*;

    fn facts() -> VersionFacts {
        VersionFacts {
            mc_version: "1.21.1".into(),
            data: DataVersion::Version { n: 3955 },
        }
    }

    fn member(root: &Path, kind: Kind, join: Join) {
        let mut s = Sidecar::default();
        s.set_kind(
            kind,
            KindState {
                enabled: true,
                join,
            },
        );
        sidecar::write(root, &s).unwrap();
    }

    fn history(root: &Path, text: &str) {
        let p = game_file(root, Kind::History);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn sidecar_of(root: &Path) -> Sidecar {
        match sidecar::read(root).unwrap() {
            SidecarRead::Member(s) => s,
            SidecarRead::NotMember => panic!("member"),
        }
    }

    fn earlier_marker(root: &Path) {
        let p = marker::path(root);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, br#"{"pid":7,"started_unix_ms":1,"session":"earlier"}"#).unwrap();
    }

    fn launch(
        inst: &Path,
        data: &Path,
        confirmed: bool,
        probe: crate::platform::ImageMatch,
    ) -> crate::error::Result<Option<VersionFacts>> {
        launch_sync(
            data,
            inst,
            &data.join("versions"),
            "1.21.1",
            confirmed,
            &move |_| probe,
        )
    }

    #[test]
    fn a_launch_of_a_non_member_checks_nothing() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        earlier_marker(&inst);
        let got = launch(&inst, &data, false, crate::platform::ImageMatch::Yes).unwrap();
        assert_eq!(got, None);
    }

    #[test]
    fn a_launch_syncs_a_member_and_returns_its_facts() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        history(&inst, "/a\n");
        member(&inst, Kind::Servers, Join::UseShared);
        let facts = launch(&inst, &data, false, crate::platform::ImageMatch::No)
            .unwrap()
            .expect("syncs");
        assert_eq!(facts.mc_version, "1.21.1");
    }

    #[test]
    fn a_game_from_before_a_restart_blocks_or_asks() {
        use crate::error::Error;
        use crate::platform::ImageMatch;
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        member(&inst, Kind::Servers, Join::UseShared);
        earlier_marker(&inst);
        assert!(matches!(
            launch(&inst, &data, false, ImageMatch::Yes),
            Err(Error::SettingsSyncGameRunning)
        ));
        assert!(matches!(
            launch(&inst, &data, false, ImageMatch::Unknown),
            Err(Error::SettingsSyncAskGameClosed)
        ));
        assert!(marker::path(&inst).exists(), "kept while unanswered");
        assert!(launch(&inst, &data, true, ImageMatch::Unknown)
            .unwrap()
            .is_some());
        assert!(!marker::path(&inst).exists(), "the user said it is closed");
    }

    #[test]
    fn a_launch_sync_failure_names_the_kind_and_file() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        member(&inst, Kind::Servers, Join::UseShared);
        let p = game_file(&inst, Kind::Servers);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"garbage").unwrap();
        match launch(&inst, &data, false, crate::platform::ImageMatch::No) {
            Err(crate::error::Error::SettingsSyncFailed {
                sync_kind,
                file,
                reason,
            }) => {
                assert_eq!(sync_kind, Some(Kind::Servers));
                assert_eq!(file, Some(SyncFile::Game), "this instance's own file");
                assert_eq!(reason, FileReason::Damaged);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_non_member_touches_nothing() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        history(&inst, "/a\n");
        before_launch(&data, &inst, &facts()).unwrap();
        assert!(!store::store_dir(&data).exists());
    }

    #[test]
    fn a_member_syncs_and_its_join_completes() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        history(&inst, "/a\n");
        member(&inst, Kind::History, Join::UseShared);
        before_launch(&data, &inst, &facts()).unwrap();
        assert!(store::store_dir(&data).join("command_history.txt").exists());
        assert_eq!(
            sidecar_of(&inst).kind(Kind::History).unwrap().join,
            Join::Done
        );
    }

    #[test]
    fn an_unreadable_sidecar_fails_naming_it() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        let p = sidecar_path(&inst);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"{").unwrap();
        let f = before_launch(&data, &inst, &facts()).unwrap_err();
        assert_eq!(f.kind, None);
        assert_eq!(f.file, Some(SyncFile::Settings));
        assert_eq!(f.reason, FileReason::Damaged);
    }

    #[test]
    fn a_damaged_shared_copy_is_named_as_the_shared_one() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        member(&inst, Kind::Servers, Join::UseShared);
        let shared = store::store_dir(&data).join("servers.dat");
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        std::fs::write(&shared, b"garbage").unwrap();
        let f = before_launch(&data, &inst, &facts()).unwrap_err();
        assert_eq!(f.kind, Some(Kind::Servers));
        assert_eq!(f.file, Some(SyncFile::Shared));
        assert_eq!(f.reason, FileReason::Damaged);
    }

    fn servers_file(root: &Path, entries: &[(&str, &str)]) {
        let mut v = crate::servers::nbt::empty_root();
        for (name, ip) in entries {
            crate::servers::nbt::push_server(&mut v, name, ip).unwrap();
        }
        let p = game_file(root, Kind::Servers);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, crate::servers::nbt::serialize(&v).unwrap()).unwrap();
    }

    fn shared_servers(data: &Path) -> Vec<String> {
        let bytes = std::fs::read(store::store_dir(data).join("servers.dat")).unwrap();
        crate::servers::nbt::list_view(&crate::servers::nbt::parse(&bytes).unwrap())
            .into_iter()
            .map(|s| s.address)
            .collect()
    }

    #[test]
    fn a_local_edit_reaches_the_shared_list_at_once() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        servers_file(&inst, &[("A", "a.example.net")]);
        member(&inst, Kind::Servers, Join::UseShared);
        before_launch(&data, &inst, &facts()).unwrap();
        servers_file(&inst, &[("A", "a.example.net"), ("B", "b.example.net")]);
        after_local_edit(&data, &inst, Kind::Servers, &no_game, facts).unwrap();
        assert_eq!(shared_servers(&data), ["a.example.net", "b.example.net"]);
    }

    fn no_game(_: u32) -> crate::platform::ImageMatch {
        crate::platform::ImageMatch::No
    }

    #[test]
    fn a_local_edit_does_not_sync_while_a_game_from_before_a_restart_may_run() {
        use crate::platform::ImageMatch;
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        servers_file(&inst, &[("A", "a.example.net")]);
        member(&inst, Kind::Servers, Join::UseShared);
        before_launch(&data, &inst, &facts()).unwrap();
        earlier_marker(&inst);
        servers_file(&inst, &[("A", "a.example.net"), ("B", "b.example.net")]);
        for probe in [ImageMatch::Yes, ImageMatch::Unknown] {
            after_local_edit(&data, &inst, Kind::Servers, &move |_| probe, facts).unwrap();
            assert_eq!(
                shared_servers(&data),
                ["a.example.net"],
                "{probe:?}: not synced"
            );
        }
        assert!(
            marker::path(&inst).exists(),
            "the question stays for the launch"
        );
    }

    #[test]
    fn a_local_edit_of_a_kind_not_synced_reads_and_touches_nothing() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        servers_file(&inst, &[("A", "a.example.net")]);
        member(&inst, Kind::History, Join::UseShared);
        let read = std::cell::Cell::new(false);
        after_local_edit(&data, &inst, Kind::Servers, &no_game, || {
            read.set(true);
            facts()
        })
        .unwrap();
        assert!(!store::store_dir(&data).join("servers.dat").exists());
        assert!(!read.get(), "no version read for a kind it does not sync");
    }

    #[test]
    fn a_local_edit_keeps_another_kinds_failure_and_records_its_own() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        servers_file(&inst, &[("A", "a.example.net")]);
        let history_failure = SyncFailure {
            kind: Some(Kind::History),
            file: Some(SyncFile::Game),
            reason: FileReason::NoAccess,
        };
        let mut s = Sidecar::default();
        for kind in [Kind::Servers, Kind::History] {
            s.set_kind(
                kind,
                KindState {
                    enabled: true,
                    join: Join::UseShared,
                },
            );
        }
        s.last_error = Some(history_failure.clone());
        sidecar::write(&inst, &s).unwrap();
        after_local_edit(&data, &inst, Kind::Servers, &no_game, facts).unwrap();
        assert_eq!(sidecar_of(&inst).last_error, Some(history_failure));
        std::fs::write(game_file(&inst, Kind::Servers), b"garbage").unwrap();
        let f = after_local_edit(&data, &inst, Kind::Servers, &no_game, facts).unwrap_err();
        assert_eq!(f.kind, Some(Kind::Servers));
        assert_eq!(sidecar_of(&inst).last_error, Some(f));
    }

    #[test]
    fn the_synced_files_are_the_enabled_kinds_and_an_unreadable_sidecar_is_an_error() {
        let td = tempfile::tempdir().unwrap();
        let inst = td.path().join("inst");
        assert!(synced_files(&inst).unwrap().is_empty(), "not a member");
        let mut s = Sidecar::default();
        for (kind, enabled) in [
            (Kind::Servers, true),
            (Kind::History, false),
            (Kind::Hotbars, true),
        ] {
            s.set_kind(
                kind,
                KindState {
                    enabled,
                    join: Join::Done,
                },
            );
        }
        sidecar::write(&inst, &s).unwrap();
        assert_eq!(synced_files(&inst).unwrap(), ["servers.dat", "hotbar.nbt"]);
        std::fs::write(sidecar_path(&inst), b"{").unwrap();
        assert!(synced_files(&inst).is_err());
    }

    #[test]
    fn an_exit_failure_is_kept_and_a_later_success_clears_it() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        history(&inst, "/a\n");
        member(&inst, Kind::History, Join::UseShared);
        before_launch(&data, &inst, &facts()).unwrap();
        std::fs::write(game_file(&inst, Kind::History), [0xff, 0xfe]).unwrap(); // not UTF-8
        let f = after_exit(&data, &inst, &facts()).unwrap_err();
        assert_eq!(f.kind, Some(Kind::History));
        assert_eq!(sidecar_of(&inst).last_error, Some(f));
        history(&inst, "/a\n/b\n");
        before_launch(&data, &inst, &facts()).unwrap();
        assert_eq!(sidecar_of(&inst).last_error, None);
    }
}

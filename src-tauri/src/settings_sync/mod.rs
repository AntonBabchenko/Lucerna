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
pub mod engine;
pub(crate) mod fsio;
pub(crate) mod history;
pub mod hotbars;
pub mod marker;
pub mod membership;
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
    match marker::check(instance_root, probe)? {
        marker::MarkerState::Clear => {}
        marker::MarkerState::Alive => return Err(Error::SettingsSyncGameRunning),
        marker::MarkerState::Unknown if game_closed_confirmed => marker::remove(instance_root)?,
        marker::MarkerState::Unknown => return Err(Error::SettingsSyncAskGameClosed),
    }
    let facts = version::VersionFacts::read(versions_dir, mc_version);
    before_launch(data_root, instance_root, &facts).map_err(|f| Error::SettingsSyncFailed {
        sync_kind: f.kind,
        file: f.file,
        reason: f.reason,
    })?;
    Ok(Some(facts))
}

fn run_all(
    data_root: &Path,
    instance_root: &Path,
    facts: &version::VersionFacts,
    keep_failure: bool,
) -> std::result::Result<(), SyncFailure> {
    let sidecar_file = || sidecar_path(instance_root).display().to_string();
    let before = match sidecar::read(instance_root) {
        Ok(sidecar::SidecarRead::NotMember) => return Ok(()),
        Ok(sidecar::SidecarRead::Member(s)) => s,
        Err(e) => {
            return Err(SyncFailure {
                kind: None,
                file: sidecar_file(),
                reason: e.to_string(),
            })
        }
    };
    let store = store::lock(data_root);
    let mut after = before.clone();
    let mut failed = None;
    for kind in Kind::ALL {
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
            Ok(
                engine::KindOutcome::Joined
                | engine::KindOutcome::Synced
                | engine::KindOutcome::Resynced,
            ) => {
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
                failed = Some(SyncFailure {
                    kind: Some(kind),
                    file: game_file(instance_root, kind).display().to_string(),
                    reason: e.to_string(),
                });
                break;
            }
        }
    }
    after.last_error = match &failed {
        Some(f) if keep_failure => Some(f.clone()),
        Some(_) => before.last_error.clone(),
        None => None,
    };
    if after != before {
        if let Err(e) = sidecar::write(instance_root, &after) {
            let f = SyncFailure {
                kind: None,
                file: sidecar_file(),
                reason: e.to_string(),
            };
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
                sync_kind, file, ..
            }) => {
                assert_eq!(sync_kind, Some(Kind::Servers));
                assert!(file.ends_with("servers.dat"), "{file}");
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
        assert!(f.file.ends_with("settings-sync.json"), "{}", f.file);
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

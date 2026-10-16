//! Joining and leaving (spec §B7), stopping a kind from the launch dialog,
//! and what the Shared data section shows.

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::baseline::{self, Slot};
use super::engine::{self, KindOutcome, Unsupported};
use super::sidecar::{self, Join, KindState, Sidecar, SidecarRead, SyncFailure};
use super::store;
use super::version::VersionFacts;
use super::{game_file, sidecar_path, Kind};
use crate::error::{Error, Result};

/// Whether this instance's version can sync a kind now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unsupported {
        why: Unsupported,
    },
    /// Shared hotbars were saved by a newer Minecraft: this instance keeps its own.
    OlderThanShared,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct KindView {
    pub kind: Kind,
    pub enabled: bool,
    pub join: Join,
    pub availability: Availability,
    /// Hotbars only: shared hotbars exist and this instance has its own, so
    /// turning sync on asks which to keep.
    pub join_asks: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SidecarStatus {
    Readable,
    /// The instance's sync settings cannot be read: nothing syncs until
    /// "Stop syncing" replaces them.
    Unreadable {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct SectionState {
    pub sidecar: SidecarStatus,
    pub kinds: Vec<KindView>,
    pub last_error: Option<SyncFailure>,
}

pub fn state(data_root: &Path, instance_root: &Path, facts: &VersionFacts) -> Result<SectionState> {
    let (sidecar, status) = match sidecar::read(instance_root) {
        Ok(SidecarRead::Member(s)) => (s, SidecarStatus::Readable),
        Ok(SidecarRead::NotMember) => (Sidecar::default(), SidecarStatus::Readable),
        Err(e) => (
            Sidecar::default(),
            SidecarStatus::Unreadable {
                reason: e.to_string(),
            },
        ),
    };
    let store = store::lock(data_root);
    let shared_hotbars = shared_hotbars_version(&store)?;
    let has_own_hotbars =
        super::fsio::read_if_present(&game_file(instance_root, Kind::Hotbars))?.is_some();
    let kinds = Kind::ALL
        .iter()
        .map(|&kind| {
            let st = sidecar.kind(kind).cloned().unwrap_or(KindState {
                enabled: false,
                join: Join::Done,
            });
            let availability = match engine::unsupported(kind, facts) {
                Some(why) => Availability::Unsupported { why },
                None if kind == Kind::Hotbars
                    && shared_hotbars
                        .0
                        .zip(facts.hotbar_version())
                        .is_some_and(|(s, mine)| s > mine) =>
                {
                    Availability::OlderThanShared
                }
                None => Availability::Available,
            };
            KindView {
                kind,
                enabled: st.enabled,
                join: st.join,
                availability,
                join_asks: kind == Kind::Hotbars && shared_hotbars.1 && has_own_hotbars,
            }
        })
        .collect();
    Ok(SectionState {
        sidecar: status,
        kinds,
        last_error: sidecar.last_error,
    })
}

/// The shared hotbars' `DataVersion`, and whether there are shared hotbars.
fn shared_hotbars_version(store: &store::StoreLock) -> Result<(Option<i32>, bool)> {
    let Some(bytes) = store.load(Kind::Hotbars)? else {
        return Ok((None, false));
    };
    let path = || store.dir().join(Kind::Hotbars.file()).display().to_string();
    let (root, framing) = crate::datapacks::level_dat::parse(&bytes)?;
    let bars = super::hotbars::from_root(root, framing).map_err(|why| Error::io(path(), why))?;
    Ok((bars.data_version, true))
}

/// Turn `kind` on (a join, completed now when the version allows) or off
/// (the instance keeps its file; its baseline goes).
pub fn set_kind(
    data_root: &Path,
    instance_root: &Path,
    facts: &VersionFacts,
    kind: Kind,
    enabled: bool,
    join: Option<Join>,
) -> Result<Option<KindOutcome>> {
    let mut sc = match sidecar::read(instance_root)? {
        SidecarRead::Member(s) => s,
        SidecarRead::NotMember => Sidecar::default(),
    };
    if !enabled {
        sc.set_kind(
            kind,
            KindState {
                enabled: false,
                join: Join::Done,
            },
        );
        sidecar::write(instance_root, &sc)?;
        forget(instance_root, kind)?;
        return Ok(None);
    }
    let join = join.unwrap_or(Join::UseShared);
    sc.set_kind(
        kind,
        KindState {
            enabled: true,
            join,
        },
    );
    sidecar::write(instance_root, &sc)?;
    let store = store::lock(data_root);
    let outcome = engine::sync_kind(
        instance_root,
        &store,
        facts,
        kind,
        join,
        &engine::Seams::none(),
    )?;
    if matches!(
        outcome,
        KindOutcome::Joined | KindOutcome::Synced | KindOutcome::Resynced
    ) {
        sc.set_kind(
            kind,
            KindState {
                enabled: true,
                join: Join::Done,
            },
        );
        sidecar::write(instance_root, &sc)?;
    }
    Ok(Some(outcome))
}

/// "Stop syncing" from the launch dialog: `kind`, or every kind when the
/// sync settings themselves could not be read. An unreadable sidecar goes to
/// the Recycle Bin and a fresh one is written with nothing on.
pub fn stop(
    instance_root: &Path,
    kind: Option<Kind>,
    recycle: &dyn Fn(PathBuf) -> io::Result<()>,
) -> Result<()> {
    let (mut sc, stopping): (Sidecar, Vec<Kind>) = match sidecar::read(instance_root) {
        Ok(SidecarRead::NotMember) => return Ok(()),
        Ok(SidecarRead::Member(s)) => (s, kind.map_or_else(|| Kind::ALL.to_vec(), |k| vec![k])),
        Err(_) => {
            let path = sidecar_path(instance_root);
            recycle(path.clone()).map_err(|e| Error::io(path.display().to_string(), e))?;
            (Sidecar::default(), Kind::ALL.to_vec())
        }
    };
    for k in &stopping {
        sc.set_kind(
            *k,
            KindState {
                enabled: false,
                join: Join::Done,
            },
        );
    }
    sc.last_error = None;
    sidecar::write(instance_root, &sc)?;
    for k in stopping {
        forget(instance_root, k)?;
    }
    Ok(())
}

/// A kind this instance left: its baseline and pending go (checked), so a
/// later join starts as a join.
fn forget(instance_root: &Path, kind: Kind) -> Result<()> {
    baseline::remove(instance_root, kind, Slot::Pending)?;
    baseline::remove(instance_root, kind, Slot::Baseline)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::settings_sync::version::DataVersion;

    fn facts(mc: &str) -> VersionFacts {
        VersionFacts {
            mc_version: mc.into(),
            data: DataVersion::Version { n: 3955 },
        }
    }

    fn put(root: &Path, kind: Kind, bytes: &[u8]) {
        let p = game_file(root, kind);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn read_sidecar(root: &Path) -> Sidecar {
        match sidecar::read(root).unwrap() {
            SidecarRead::Member(s) => s,
            SidecarRead::NotMember => panic!("member"),
        }
    }

    #[test]
    fn a_non_member_sees_every_kind_off_with_its_availability() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        let s = state(&data, &inst, &facts("1.20.1")).unwrap();
        assert_eq!(s.sidecar, SidecarStatus::Readable);
        let history = s.kinds.iter().find(|k| k.kind == Kind::History).unwrap();
        assert!(!history.enabled);
        assert_eq!(
            history.availability,
            Availability::Unsupported {
                why: Unsupported::HistoryTooOld
            }
        );
        assert_eq!(s.kinds.len(), Kind::ALL.len());
    }

    #[test]
    fn turning_a_kind_on_joins_now() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        put(&inst, Kind::History, b"/a\n");
        let got = set_kind(&data, &inst, &facts("1.21.1"), Kind::History, true, None).unwrap();
        assert_eq!(got, Some(KindOutcome::Joined));
        let s = read_sidecar(&inst);
        assert_eq!(
            s.kind(Kind::History),
            Some(&KindState {
                enabled: true,
                join: Join::Done
            })
        );
        assert!(store::store_dir(&data).join("command_history.txt").exists());
    }

    #[test]
    fn a_kind_the_version_cannot_sync_waits_on() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        set_kind(&data, &inst, &facts("1.20.1"), Kind::History, true, None).unwrap();
        assert_eq!(
            read_sidecar(&inst).kind(Kind::History).map(|k| k.join),
            Some(Join::UseShared)
        );
    }

    #[test]
    fn turning_a_kind_off_keeps_the_file_and_drops_the_baseline() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        put(&inst, Kind::History, b"/a\n");
        set_kind(&data, &inst, &facts("1.21.1"), Kind::History, true, None).unwrap();
        set_kind(&data, &inst, &facts("1.21.1"), Kind::History, false, None).unwrap();
        assert!(!read_sidecar(&inst).enabled(Kind::History));
        assert!(game_file(&inst, Kind::History).exists());
        assert_eq!(
            baseline::read(&inst, Kind::History, Slot::Baseline).unwrap(),
            None
        );
    }

    #[test]
    fn joining_hotbars_asks_when_both_sides_have_some() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        let bars = crate::settings_sync::hotbars::Hotbars {
            rows: [("0".to_string(), fastnbt::Value::List(vec![]))]
                .into_iter()
                .collect(),
            data_version: Some(3955),
            framing: crate::datapacks::level_dat::Framing::Raw,
        };
        let bytes = crate::settings_sync::hotbars::to_bytes(&bars).unwrap();
        put(&inst, Kind::Hotbars, &bytes);
        let view = |s: &SectionState| {
            s.kinds
                .iter()
                .find(|k| k.kind == Kind::Hotbars)
                .unwrap()
                .join_asks
        };
        assert!(!view(&state(&data, &inst, &facts("1.21.1")).unwrap()));
        store::lock(&data).save(Kind::Hotbars, &bytes).unwrap();
        assert!(view(&state(&data, &inst, &facts("1.21.1")).unwrap()));
    }

    #[test]
    fn an_unreadable_sidecar_is_shown_and_refuses_a_change() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        let p = sidecar_path(&inst);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"{").unwrap();
        assert!(matches!(
            state(&data, &inst, &facts("1.21.1")).unwrap().sidecar,
            SidecarStatus::Unreadable { .. }
        ));
        assert!(set_kind(&data, &inst, &facts("1.21.1"), Kind::Servers, true, None).is_err());
    }

    #[test]
    fn stopping_with_an_unreadable_sidecar_recycles_it_and_starts_over() {
        let td = tempfile::tempdir().unwrap();
        let inst = td.path().join("inst");
        let p = sidecar_path(&inst);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"{").unwrap();
        let binned = RefCell::new(Vec::new());
        let recycle = |path: PathBuf| {
            binned.borrow_mut().push(path.clone());
            std::fs::remove_file(path)
        };
        stop(&inst, None, &recycle).unwrap();
        assert_eq!(binned.borrow().as_slice(), [p.clone()]);
        let s = read_sidecar(&inst);
        assert!(Kind::ALL.iter().all(|k| !s.enabled(*k)));
    }

    #[test]
    fn stopping_one_kind_leaves_the_others() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        set_kind(&data, &inst, &facts("1.21.1"), Kind::Servers, true, None).unwrap();
        set_kind(&data, &inst, &facts("1.21.1"), Kind::History, true, None).unwrap();
        stop(&inst, Some(Kind::Servers), &|_| Ok(())).unwrap();
        let s = read_sidecar(&inst);
        assert!(!s.enabled(Kind::Servers));
        assert!(s.enabled(Kind::History));
    }
}

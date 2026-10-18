//! Joining and leaving (spec §B7), stopping a kind from the launch dialog,
//! and what the Shared data section shows.

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::baseline::{self, Slot};
use super::engine::{self, KindOutcome, Unsupported};
use super::failure::At;
use super::options::doc::{self as options_doc, OptionsRead, WaitReason};
use super::sidecar::{self, Join, KindState, Sidecar, SidecarRead, SyncFailure};
use super::store;
use super::version::VersionFacts;
use super::{game_file, sidecar_path, Kind};
use crate::error::Result;

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
    /// options.txt is not this game's own yet (spec §C4): nothing syncs
    /// until the game has written it.
    Waiting {
        why: WaitReason,
    },
    /// A file this row needs cannot be read; the other rows still work.
    Unreadable {
        file: super::failure::SyncFile,
        reason: super::failure::FileReason,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct KindView {
    pub kind: Kind,
    pub enabled: bool,
    pub join: Join,
    pub availability: Availability,
    /// Hotbars and options: shared ones exist and this instance has its
    /// own, so turning sync on asks which to keep.
    pub join_asks: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SidecarStatus {
    Readable,
    /// The instance's sync settings cannot be read: nothing syncs until
    /// "Stop syncing" replaces them.
    Unreadable {
        reason: super::failure::FileReason,
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
        Err(e) => {
            crate::diag!("settings sync: the sync settings cannot be read ({e})");
            (
                Sidecar::default(),
                SidecarStatus::Unreadable {
                    reason: super::failure::classify(&e).1,
                },
            )
        }
    };
    let store = store::lock(data_root);
    // A hotbars file that cannot be read makes the hotbars row say so; the
    // other rows, and the way out, stay usable.
    let hotbars_files = shared_hotbars_version(&store).and_then(|shared| {
        let own = super::fsio::read_if_present(
            At::game(Kind::Hotbars),
            &game_file(instance_root, Kind::Hotbars),
        )?;
        Ok((shared, own.is_some()))
    });
    let (shared_hotbars, has_own_hotbars, hotbars_unreadable) = match hotbars_files {
        Ok((shared, own)) => (shared, own, None),
        Err(e) => {
            crate::diag!("settings sync: the hotbars cannot be read ({e})");
            let (file, reason) = super::failure::classify(&e);
            let unreadable = Availability::Unreadable {
                file: file.unwrap_or(super::failure::SyncFile::Shared),
                reason,
            };
            ((None, false), false, Some(unreadable))
        }
    };
    let options = options_row(&store, instance_root, facts);
    let kinds = Kind::ALL
        .iter()
        .map(|&kind| {
            let st = sidecar.kind(kind).cloned().unwrap_or(KindState {
                enabled: false,
                join: Join::Done,
            });
            let availability = match engine::unsupported(kind, facts) {
                Some(why) => Availability::Unsupported { why },
                None if kind == Kind::Options => options.0,
                None if kind == Kind::Hotbars && hotbars_unreadable.is_some() => {
                    hotbars_unreadable.unwrap_or(Availability::Available)
                }
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
                join_asks: match kind {
                    Kind::Hotbars => shared_hotbars.1 && has_own_hotbars,
                    Kind::Options => options.1,
                    Kind::Servers | Kind::History => false,
                },
            }
        })
        .collect();
    Ok(SectionState {
        sidecar: status,
        kinds,
        last_error: sidecar.last_error,
    })
}

/// The options row (for a version the catalog serves): waiting until the
/// game has written this instance's options.txt, unreadable when it or the
/// shared copy cannot be read, else available, and a join asks when shared
/// settings exist. The other rows stay usable either way.
fn options_row(
    store: &store::StoreLock,
    instance_root: &Path,
    facts: &VersionFacts,
) -> (Availability, bool) {
    let Ok(dv) = facts.options_version() else {
        // `engine::unsupported` names the reason for this row.
        return (Availability::Available, false);
    };
    let files = shared_options_exist(store).and_then(|shared| {
        let own = options_doc::read(
            At::game(Kind::Options),
            &game_file(instance_root, Kind::Options),
            dv,
        )?;
        Ok((shared, own))
    });
    match files {
        Ok((_, OptionsRead::Waiting { why, .. })) => (Availability::Waiting { why }, false),
        Ok((shared, OptionsRead::File(_))) => (Availability::Available, shared),
        Err(e) => {
            crate::diag!("settings sync: the game options cannot be read ({e})");
            let (file, reason) = super::failure::classify(&e);
            let unreadable = Availability::Unreadable {
                file: file.unwrap_or(super::failure::SyncFile::Shared),
                reason,
            };
            (unreadable, false)
        }
    }
}

/// Whether there are shared settings; a shared copy that does not parse is
/// an error naming it.
fn shared_options_exist(store: &store::StoreLock) -> Result<bool> {
    let Some(bytes) = store.load(Kind::Options)? else {
        return Ok(false);
    };
    options_doc::parse_store(&bytes).map_err(|why| {
        super::failure::damaged(
            At::shared(Kind::Options),
            &store.dir().join(Kind::Options.store_file()),
            why,
        )
    })?;
    Ok(true)
}

/// The shared hotbars' `DataVersion`, and whether there are shared hotbars.
fn shared_hotbars_version(store: &store::StoreLock) -> Result<(Option<i32>, bool)> {
    let Some(bytes) = store.load(Kind::Hotbars)? else {
        return Ok((None, false));
    };
    let path = store.dir().join(Kind::Hotbars.file());
    let at = At::shared(Kind::Hotbars);
    let (root, framing) = crate::datapacks::level_dat::parse(&bytes)
        .map_err(|e| super::failure::damaged(at, &path, e))?;
    let bars = super::hotbars::from_root(root, framing)
        .map_err(|why| super::failure::damaged(at, &path, why))?;
    Ok((bars.data_version, true))
}

/// The section's toggle: [`set_kind`] behind the check every sync point
/// makes. A join writes the instance's files, which a game Lucerna lost track
/// of would rewrite when it closes, dropping what the join brought in — and
/// the next sync point would then remove those entries from the shared copy.
/// Leaving writes no game file, so it never waits.
#[allow(clippy::too_many_arguments)]
pub fn join_or_leave(
    data_root: &Path,
    instance_root: &Path,
    facts: &VersionFacts,
    kind: Kind,
    enabled: bool,
    join: Option<Join>,
    game_closed_confirmed: bool,
    probe: &dyn Fn(u32) -> crate::platform::ImageMatch,
) -> Result<Option<KindOutcome>> {
    if enabled {
        super::guard_orphan(instance_root, game_closed_confirmed, probe)?;
    }
    set_kind(data_root, instance_root, facts, kind, enabled, join)
}

/// Turn `kind` on (a join, completed now when the version allows) or off
/// (the instance keeps its file; its baseline goes).
fn set_kind(
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
        // The records first: if they cannot be removed the kind stays on, as
        // it was, rather than off with a baseline that would make a later
        // join look like an ordinary sync.
        forget(instance_root, kind)?;
        sc.set_kind(
            kind,
            KindState {
                enabled: false,
                join: Join::Done,
            },
        );
        clear_failure_of(&mut sc, &[kind]);
        sidecar::write(instance_root, &sc)?;
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
    if matches!(outcome, KindOutcome::Joined | KindOutcome::Synced) {
        sc.set_kind(
            kind,
            KindState {
                enabled: true,
                join: Join::Done,
            },
        );
        clear_failure_of(&mut sc, &[kind]);
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
            recycle(path.clone()).map_err(|e| super::failure::io(At::settings(), &path, e))?;
            (Sidecar::default(), Kind::ALL.to_vec())
        }
    };
    for k in &stopping {
        forget(instance_root, *k)?;
    }
    for k in &stopping {
        sc.set_kind(
            *k,
            KindState {
                enabled: false,
                join: Join::Done,
            },
        );
    }
    clear_failure_of(&mut sc, &stopping);
    sidecar::write(instance_root, &sc)?;
    Ok(())
}

/// A kept failure about one of `kinds`, or about the sync settings
/// themselves, no longer applies once those kinds stopped or synced; one
/// about another kind stays until that kind syncs.
pub(super) fn clear_failure_of(sc: &mut Sidecar, kinds: &[Kind]) {
    if sc
        .last_error
        .as_ref()
        .is_some_and(|f| f.kind.is_none_or(|k| kinds.contains(&k)))
    {
        sc.last_error = None;
    }
}

/// A kind this instance left: its baseline and pending go (checked), so a
/// later join starts as a join.
fn forget(instance_root: &Path, kind: Kind) -> Result<()> {
    engine::forget_records(instance_root, kind)
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
            release: Some(true),
            loader: crate::instances::schema::LoaderKind::Vanilla,
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
    fn a_join_waits_for_a_game_from_before_a_restart_but_leaving_does_not() {
        use crate::error::Error;
        use crate::platform::ImageMatch;
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        put(&inst, Kind::History, b"/a\n");
        let earlier = || {
            let p = super::super::marker::path(&inst);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, br#"{"pid":7,"started_unix_ms":1,"session":"earlier"}"#).unwrap();
        };
        let f = facts("1.21.1");
        let join = |confirmed: bool, probe: ImageMatch| {
            join_or_leave(
                &data,
                &inst,
                &f,
                Kind::History,
                true,
                None,
                confirmed,
                &move |_| probe,
            )
        };
        earlier();
        assert!(matches!(
            join(false, ImageMatch::Yes),
            Err(Error::SettingsSyncGameRunning)
        ));
        assert!(matches!(
            join(false, ImageMatch::Unknown),
            Err(Error::SettingsSyncAskGameClosed)
        ));
        assert!(
            !read_sidecar_or_none(&inst),
            "nothing joined while it may run"
        );
        assert_eq!(
            join(true, ImageMatch::Unknown).unwrap(),
            Some(KindOutcome::Joined)
        );
        assert!(
            !super::super::marker::path(&inst).exists(),
            "the user said it is closed"
        );
        earlier();
        let left = join_or_leave(&data, &inst, &f, Kind::History, false, None, false, &|_| {
            ImageMatch::Yes
        });
        assert_eq!(left.unwrap(), None, "leaving writes no game file");
    }

    fn read_sidecar_or_none(root: &Path) -> bool {
        matches!(sidecar::read(root).unwrap(), SidecarRead::Member(_))
    }

    const OPTIONS: &[u8] = b"version:3955\nfov:0.5\nlang:ru_ru\n";

    fn options_view(data: &Path, inst: &Path) -> KindView {
        state(data, inst, &facts("1.21.1"))
            .unwrap()
            .kinds
            .into_iter()
            .find(|k| k.kind == Kind::Options)
            .unwrap()
    }

    #[test]
    fn the_options_row_waits_for_a_file_the_game_wrote_and_says_why() {
        use crate::settings_sync::failure::{FileReason, SyncFile};
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        assert_eq!(
            options_view(&data, &inst).availability,
            Availability::Waiting {
                why: WaitReason::FirstLaunch
            }
        );
        put(&inst, Kind::Options, b"fov:0.5\n");
        assert_eq!(
            options_view(&data, &inst).availability,
            Availability::Waiting {
                why: WaitReason::GameSave
            }
        );
        put(&inst, Kind::Options, OPTIONS);
        assert_eq!(
            options_view(&data, &inst).availability,
            Availability::Available
        );
        put(&inst, Kind::Options, b"version:3955\nlang:\xff\n");
        assert_eq!(
            options_view(&data, &inst).availability,
            Availability::Unreadable {
                file: SyncFile::Game,
                reason: FileReason::Damaged
            }
        );
    }

    #[test]
    fn turning_options_on_asks_only_when_both_sides_have_settings() {
        let td = tempfile::tempdir().unwrap();
        let (data, a, b) = (
            td.path().join("root"),
            td.path().join("a"),
            td.path().join("b"),
        );
        put(&a, Kind::Options, OPTIONS);
        assert!(!options_view(&data, &a).join_asks, "no shared settings yet");
        set_kind(&data, &a, &facts("1.21.1"), Kind::Options, true, None).unwrap();
        assert!(
            !options_view(&data, &b).join_asks,
            "b has no options.txt yet"
        );
        put(&b, Kind::Options, b"version:3955\nfov:0.9\n");
        assert!(options_view(&data, &b).join_asks);
    }

    #[test]
    fn an_options_join_before_the_first_launch_waits_and_stays_pending() {
        let td = tempfile::tempdir().unwrap();
        let (data, a, b) = (
            td.path().join("root"),
            td.path().join("a"),
            td.path().join("b"),
        );
        put(&a, Kind::Options, OPTIONS);
        set_kind(&data, &a, &facts("1.21.1"), Kind::Options, true, None).unwrap();
        let got = set_kind(&data, &b, &facts("1.21.1"), Kind::Options, true, None).unwrap();
        assert_eq!(
            got,
            Some(KindOutcome::Waiting {
                why: WaitReason::FirstLaunch
            })
        );
        assert_eq!(
            read_sidecar(&b).kind(Kind::Options).map(|k| k.join),
            Some(Join::UseShared)
        );
        assert!(!game_file(&b, Kind::Options).exists());
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

    fn failure(kind: Kind) -> SyncFailure {
        SyncFailure {
            kind: Some(kind),
            file: Some(super::super::failure::SyncFile::Game),
            reason: super::super::failure::FileReason::NoAccess,
        }
    }

    fn keep_failure(root: &Path, kind: Kind) {
        let mut s = read_sidecar(root);
        s.last_error = Some(failure(kind));
        sidecar::write(root, &s).unwrap();
    }

    #[test]
    fn leaving_a_kind_clears_its_kept_failure_but_not_anothers() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        let f = facts("1.21.1");
        set_kind(&data, &inst, &f, Kind::Servers, true, None).unwrap();
        set_kind(&data, &inst, &f, Kind::History, true, None).unwrap();
        keep_failure(&inst, Kind::History);
        set_kind(&data, &inst, &f, Kind::Servers, false, None).unwrap();
        assert_eq!(read_sidecar(&inst).last_error, Some(failure(Kind::History)));
        set_kind(&data, &inst, &f, Kind::History, false, None).unwrap();
        assert_eq!(read_sidecar(&inst).last_error, None);
    }

    #[test]
    fn stopping_one_kind_keeps_another_kinds_failure() {
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        set_kind(&data, &inst, &facts("1.21.1"), Kind::Servers, true, None).unwrap();
        keep_failure(&inst, Kind::History);
        stop(&inst, Some(Kind::Servers), &|_| Ok(())).unwrap();
        assert_eq!(read_sidecar(&inst).last_error, Some(failure(Kind::History)));
    }

    #[test]
    fn a_leave_whose_records_cannot_be_removed_stays_a_member() {
        // Removing the baseline first: a failure leaves the kind on, as it
        // was, instead of off with a stale baseline that would make a later
        // join look like an ordinary sync.
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        put(&inst, Kind::History, b"/a\n");
        set_kind(&data, &inst, &facts("1.21.1"), Kind::History, true, None).unwrap();
        let base = baseline::path(&inst, Kind::History, Slot::Baseline);
        std::fs::remove_file(&base).unwrap();
        std::fs::create_dir(&base).unwrap(); // cannot be removed as a file
        assert!(set_kind(&data, &inst, &facts("1.21.1"), Kind::History, false, None).is_err());
        assert!(read_sidecar(&inst).enabled(Kind::History));
    }

    #[test]
    fn a_damaged_shared_hotbars_file_degrades_only_the_hotbars_row() {
        use super::super::failure::{FileReason, SyncFile};
        let td = tempfile::tempdir().unwrap();
        let (data, inst) = (td.path().join("root"), td.path().join("inst"));
        let shared = store::store_dir(&data).join("hotbar.nbt");
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        std::fs::write(&shared, b"garbage").unwrap();
        let s = state(&data, &inst, &facts("1.21.1")).unwrap();
        let row = |k: Kind| s.kinds.iter().find(|v| v.kind == k).unwrap().availability;
        assert_eq!(row(Kind::Servers), Availability::Available);
        assert_eq!(
            row(Kind::Hotbars),
            Availability::Unreadable {
                file: SyncFile::Shared,
                reason: FileReason::Damaged
            }
        );
    }
}

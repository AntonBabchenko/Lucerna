//! Ways out of a sync file that does not parse (spec §15): the shared copy
//! replaced by this instance's, this instance's file replaced by the shared
//! copy, or this instance's records started afresh. Each is what the UI offers
//! for its reason class; none is ever run without the user asking.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::engine::{self, KindOutcome, Seams};
use super::sidecar::{self, Join, KindState, SidecarRead};
use super::version::VersionFacts;
use super::{membership, store, Kind};
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Repair {
    /// This instance's file becomes the shared copy; the damaged copy is kept
    /// in the shared folder's backups.
    ReplaceShared,
    /// This instance's file goes to the Recycle Bin and the shared copy is
    /// taken (options: once the game has written a new file).
    ReplaceOwn,
    /// This instance's records of the last sync start afresh; its file is
    /// joined with the shared copy as on turning the kind on.
    ResetRecords,
}

/// Run `repair` for `kind`. `None` when this instance does not sync `kind`.
pub fn repair(
    data_root: &Path,
    instance_root: &Path,
    facts: &VersionFacts,
    kind: Kind,
    repair: Repair,
    set_aside: &dyn Fn(PathBuf) -> io::Result<()>,
) -> Result<Option<KindOutcome>> {
    let mut sc = match sidecar::read(instance_root)? {
        SidecarRead::Member(s) if s.enabled(kind) => s,
        SidecarRead::Member(_) | SidecarRead::NotMember => return Ok(None),
    };
    let store = store::lock(data_root);
    let outcome = match repair {
        Repair::ReplaceShared => engine::share_own(instance_root, &store, facts, kind)?,
        Repair::ReplaceOwn | Repair::ResetRecords => {
            if repair == Repair::ReplaceOwn {
                engine::set_aside_damaged(instance_root, kind, set_aside)?;
            }
            engine::forget_records(instance_root, kind)?;
            set_join(instance_root, &mut sc, kind, Join::UseShared)?;
            let seams = Seams {
                set_aside,
                ..Seams::none()
            };
            engine::sync_kind(instance_root, &store, facts, kind, Join::UseShared, &seams)?
        }
    };
    if matches!(outcome, KindOutcome::Joined | KindOutcome::Synced) {
        membership::clear_failure_of(&mut sc, &[kind]);
        set_join(instance_root, &mut sc, kind, Join::Done)?;
    }
    Ok(Some(outcome))
}

fn set_join(instance_root: &Path, sc: &mut sidecar::Sidecar, kind: Kind, join: Join) -> Result<()> {
    sc.set_kind(
        kind,
        KindState {
            enabled: true,
            join,
        },
    );
    sidecar::write(instance_root, sc)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::instances::schema::LoaderKind;
    use crate::settings_sync::engine::{sync_kind, Seams};
    use crate::settings_sync::options::doc;
    use crate::settings_sync::sidecar::{self, Join, KindState, Sidecar, SidecarRead};
    use crate::settings_sync::version::DataVersion;
    use crate::settings_sync::{game_file, store};

    fn facts() -> VersionFacts {
        VersionFacts {
            mc_version: "1.21.1".into(),
            data: DataVersion::Version { n: 3955 },
            release: Some(true),
            loader: LoaderKind::Vanilla,
        }
    }

    fn put(root: &Path, kind: Kind, bytes: &[u8]) {
        let p = game_file(root, kind);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    /// `root` syncs `kind`, joined.
    fn member(data: &Path, root: &Path, kind: Kind) {
        let mut s = Sidecar::default();
        s.set_kind(
            kind,
            KindState {
                enabled: true,
                join: Join::Done,
            },
        );
        sidecar::write(root, &s).unwrap();
        let st = store::lock(data);
        let aside = |_: PathBuf| -> io::Result<()> { Ok(()) };
        let seams = Seams {
            set_aside: &aside,
            ..Seams::none()
        };
        sync_kind(root, &st, &facts(), kind, Join::UseShared, &seams).unwrap();
    }

    fn join_of(root: &Path, kind: Kind) -> Option<Join> {
        match sidecar::read(root).unwrap() {
            SidecarRead::Member(s) => s.kind(kind).map(|k| k.join),
            SidecarRead::NotMember => None,
        }
    }

    struct Bin(RefCell<Vec<String>>, PathBuf);

    impl Bin {
        fn new(dir: PathBuf) -> Self {
            Bin(RefCell::new(Vec::new()), dir)
        }
        fn take(&self, p: PathBuf) -> io::Result<()> {
            std::fs::create_dir_all(&self.1)?;
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            std::fs::rename(&p, self.1.join(&name))?;
            self.0.borrow_mut().push(name);
            Ok(())
        }
    }

    const OPTIONS_A: &[u8] = b"version:3955\nfov:0.25\nlang:ru_ru\n";

    #[test]
    fn replace_shared_puts_this_instances_file_in_place_of_a_damaged_copy() {
        let td = tempfile::tempdir().unwrap();
        let (data, a) = (td.path().join("root"), td.path().join("a"));
        put(&a, Kind::Options, OPTIONS_A);
        member(&data, &a, Kind::Options);
        let shared = store::store_dir(&data).join("options.json");
        std::fs::write(&shared, b"{damaged").unwrap();
        let bin = Bin::new(td.path().join("bin"));
        let got = repair(
            &data,
            &a,
            &facts(),
            Kind::Options,
            Repair::ReplaceShared,
            &|p| bin.take(p),
        )
        .unwrap();
        assert_eq!(got, Some(KindOutcome::Synced));
        let values = doc::parse_store(&std::fs::read(&shared).unwrap()).unwrap();
        assert_eq!(
            values.get("lang"),
            Some(&super::super::options::catalog::Neutral::Text(
                "ru_ru".into()
            ))
        );
        assert_eq!(
            std::fs::read(
                store::store_dir(&data)
                    .join("backups")
                    .join("options.json.1")
            )
            .unwrap(),
            b"{damaged",
            "the damaged copy is kept"
        );
        assert!(bin.0.borrow().is_empty());
        let st = store::lock(&data);
        let again =
            sync_kind(&a, &st, &facts(), Kind::Options, Join::Done, &Seams::none()).unwrap();
        assert_eq!(
            again,
            KindOutcome::Synced,
            "this instance syncs on as usual"
        );
    }

    #[test]
    fn replace_own_sets_a_damaged_server_list_aside_and_takes_the_shared_one() {
        let td = tempfile::tempdir().unwrap();
        let (data, a, b) = (
            td.path().join("root"),
            td.path().join("a"),
            td.path().join("b"),
        );
        let list = |names: &[&str]| {
            let entries: Vec<_> = names
                .iter()
                .map(|n| {
                    let mut m = std::collections::HashMap::new();
                    m.insert("name".to_string(), fastnbt::Value::String((*n).into()));
                    m.insert("ip".to_string(), fastnbt::Value::String(n.to_lowercase()));
                    m
                })
                .collect();
            super::super::servers::to_bytes(&entries).unwrap()
        };
        put(&a, Kind::Servers, &list(&["A", "B"]));
        member(&data, &a, Kind::Servers);
        put(&b, Kind::Servers, &list(&["A", "B"]));
        member(&data, &b, Kind::Servers);
        put(&b, Kind::Servers, b"not nbt");
        let bin = Bin::new(td.path().join("bin"));
        let got = repair(
            &data,
            &b,
            &facts(),
            Kind::Servers,
            Repair::ReplaceOwn,
            &|p| bin.take(p),
        )
        .unwrap();
        assert_eq!(got, Some(KindOutcome::Joined));
        assert_eq!(*bin.0.borrow(), ["servers (damaged).dat"]);
        assert_eq!(
            std::fs::read(td.path().join("bin").join("servers (damaged).dat")).unwrap(),
            b"not nbt"
        );
        let names: Vec<String> = super::super::servers::read(&game_file(&b, Kind::Servers))
            .unwrap()
            .unwrap_or_default()
            .iter()
            .filter_map(|e| match e.get("name") {
                Some(fastnbt::Value::String(s)) => Some(s.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["A", "B"], "the shared list, written afresh");
        assert_eq!(join_of(&b, Kind::Servers), Some(Join::Done));
    }

    #[test]
    fn replace_own_options_waits_for_the_game_to_write_a_new_file() {
        let td = tempfile::tempdir().unwrap();
        let (data, a, b) = (
            td.path().join("root"),
            td.path().join("a"),
            td.path().join("b"),
        );
        put(&a, Kind::Options, OPTIONS_A);
        member(&data, &a, Kind::Options);
        put(&b, Kind::Options, b"version:3955\nlang:\xff\n");
        let bin = Bin::new(td.path().join("bin"));
        let mut s = Sidecar::default();
        s.set_kind(
            Kind::Options,
            KindState {
                enabled: true,
                join: Join::Done,
            },
        );
        sidecar::write(&b, &s).unwrap();
        let got = repair(
            &data,
            &b,
            &facts(),
            Kind::Options,
            Repair::ReplaceOwn,
            &|p| bin.take(p),
        )
        .unwrap();
        assert_eq!(
            got,
            Some(KindOutcome::Waiting {
                why: doc::WaitReason::FirstLaunch
            })
        );
        assert_eq!(*bin.0.borrow(), ["options (damaged).txt"]);
        assert!(!game_file(&b, Kind::Options).exists(), "never created");
        assert_eq!(
            join_of(&b, Kind::Options),
            Some(Join::UseShared),
            "joins once the game wrote one"
        );
    }

    #[test]
    fn reset_records_starts_this_instances_records_afresh() {
        let td = tempfile::tempdir().unwrap();
        let (data, a) = (td.path().join("root"), td.path().join("a"));
        put(&a, Kind::Options, OPTIONS_A);
        member(&data, &a, Kind::Options);
        let base =
            super::super::baseline::path(&a, Kind::Options, super::super::baseline::Slot::Baseline);
        std::fs::write(&base, b"not nbt").unwrap();
        let st = store::lock(&data);
        assert!(sync_kind(&a, &st, &facts(), Kind::Options, Join::Done, &Seams::none()).is_err());
        drop(st);
        let bin = Bin::new(td.path().join("bin"));
        let got = repair(
            &data,
            &a,
            &facts(),
            Kind::Options,
            Repair::ResetRecords,
            &|p| bin.take(p),
        )
        .unwrap();
        assert_eq!(got, Some(KindOutcome::Joined));
        assert_eq!(
            std::fs::read(game_file(&a, Kind::Options)).unwrap(),
            OPTIONS_A
        );
        assert!(bin.0.borrow().is_empty());
    }

    #[test]
    fn a_kind_this_instance_does_not_sync_is_left_alone() {
        let td = tempfile::tempdir().unwrap();
        let (data, a) = (td.path().join("root"), td.path().join("a"));
        put(&a, Kind::Options, OPTIONS_A);
        let got = repair(
            &data,
            &a,
            &facts(),
            Kind::Options,
            Repair::ReplaceShared,
            &|_| Ok(()),
        )
        .unwrap();
        assert_eq!(got, None);
        assert!(!store::store_dir(&data).join("options.json").exists());
    }
}

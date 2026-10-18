//! options.txt through the sync point (spec §C4): two instances, the shared
//! `options.json`, the waiting states, joins and the version gate.

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};

use super::catalog::Neutral;
use super::doc::{self, Values, WaitReason};
use crate::instances::schema::LoaderKind;
use crate::settings_sync::engine::{sync_kind, KindOutcome, Seams, Unsupported};
use crate::settings_sync::failure::{classify, FileReason, SyncFile};
use crate::settings_sync::sidecar::Join;
use crate::settings_sync::store;
use crate::settings_sync::version::{DataVersion, VersionFacts};
use crate::settings_sync::{game_file, Kind};

struct World {
    _td: tempfile::TempDir,
    data_root: PathBuf,
    a: PathBuf,
    b: PathBuf,
    bin: PathBuf,
    set_aside: RefCell<Vec<String>>,
}

fn world() -> World {
    let td = tempfile::tempdir().unwrap();
    let p = td.path().to_path_buf();
    World {
        data_root: p.join("root"),
        a: p.join("a"),
        b: p.join("b"),
        bin: p.join("bin"),
        _td: td,
        set_aside: RefCell::new(Vec::new()),
    }
}

fn facts(mc: &str, dv: i32) -> VersionFacts {
    VersionFacts {
        mc_version: mc.into(),
        data: DataVersion::Version { n: dv },
        release: Some(true),
        loader: LoaderKind::Vanilla,
    }
}

fn put(root: &Path, text: &str) {
    let p = game_file(root, Kind::Options);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn file_of(root: &Path) -> Option<String> {
    std::fs::read_to_string(game_file(root, Kind::Options)).ok()
}

fn shared(w: &World) -> Option<Values> {
    let s = store::lock(&w.data_root);
    s.load(Kind::Options)
        .unwrap()
        .map(|b| doc::parse_store(&b).unwrap())
}

fn key(token: &str) -> Neutral {
    Neutral::Key {
        token: token.into(),
        modifier: None,
    }
}

impl World {
    fn sync_as(
        &self,
        root: &Path,
        f: &VersionFacts,
        join: Join,
    ) -> crate::error::Result<KindOutcome> {
        let bin = self.bin.clone();
        let log = &self.set_aside;
        let aside = move |p: PathBuf| -> io::Result<()> {
            std::fs::create_dir_all(&bin)?;
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            std::fs::rename(&p, bin.join(&name))?;
            log.borrow_mut().push(name);
            Ok(())
        };
        let seams = Seams {
            set_aside: &aside,
            ..Seams::none()
        };
        let s = store::lock(&self.data_root);
        sync_kind(root, &s, f, Kind::Options, join, &seams)
    }

    fn sync(&self, root: &Path, mc: &str, dv: i32) -> KindOutcome {
        self.sync_as(root, &facts(mc, dv), Join::UseShared).unwrap()
    }
}

const A_1_21_1: &str =
    "version:3955\nfov:0.25\nlang:ru_ru\nkey_key.jump:key.keyboard.space\nrenderDistance:24\n";

#[test]
fn an_absent_file_waits_for_the_first_launch_and_is_never_created() {
    let w = world();
    put(&w.a, A_1_21_1);
    w.sync(&w.a, "1.21.1", 3955);
    assert_eq!(
        w.sync(&w.b, "1.21.1", 3955),
        KindOutcome::Waiting {
            why: WaitReason::FirstLaunch
        }
    );
    assert_eq!(file_of(&w.b), None, "Lucerna never creates options.txt");
}

#[test]
fn a_file_this_game_did_not_write_waits_and_nothing_is_captured() {
    let w = world();
    put(&w.a, A_1_21_1);
    w.sync(&w.a, "1.21.1", 3955);
    let pack = "fov:0.9\nlang:de_de\n";
    put(&w.b, pack);
    assert_eq!(
        w.sync(&w.b, "1.21.1", 3955),
        KindOutcome::Waiting {
            why: WaitReason::GameSave
        }
    );
    assert_eq!(file_of(&w.b).as_deref(), Some(pack));
    let older = "version:3837\nfov:0.9\n";
    put(&w.b, older);
    assert_eq!(
        w.sync(&w.b, "1.21.1", 3955),
        KindOutcome::Waiting {
            why: WaitReason::GameSave
        },
        "a file of the version before an upgrade"
    );
    assert_eq!(file_of(&w.b).as_deref(), Some(older));
    let s = shared(&w).unwrap();
    assert_eq!(s.get("fov"), Some(&Neutral::Float("0.25".into())));
    assert_eq!(s.get("lang"), Some(&Neutral::Text("ru_ru".into())));
}

#[test]
fn the_first_member_seeds_the_shared_settings_and_keeps_its_file() {
    let w = world();
    put(&w.a, A_1_21_1);
    assert_eq!(w.sync(&w.a, "1.21.1", 3955), KindOutcome::Joined);
    assert_eq!(file_of(&w.a).as_deref(), Some(A_1_21_1));
    let s = shared(&w).unwrap();
    assert_eq!(s.get("key_key.jump"), Some(&key("key.keyboard.space")));
    assert_eq!(s.get("renderDistance"), None, "a machine setting stays");
}

#[test]
fn a_binding_crosses_from_glfw_to_sdl3_and_back_only_where_the_token_exists() {
    let w = world();
    put(&w.a, "version:4903\nkey_key.jump:key.keyboard.space\n");
    w.sync(&w.a, "26.2", 4903);
    put(&w.b, "version:5023\nkey_key.jump:key.keyboard.space\n");
    w.sync(&w.b, "26.3", 5023);
    put(&w.b, "version:5023\nkey_key.jump:key.keyboard.j\n");
    w.sync(&w.b, "26.3", 5023);
    w.sync(&w.a, "26.2", 4903);
    assert_eq!(
        file_of(&w.a).as_deref(),
        Some("version:4903\nkey_key.jump:key.keyboard.j\n")
    );
    put(&w.b, "version:5023\nkey_key.jump:key.keyboard.media.play\n");
    w.sync(&w.b, "26.3", 5023);
    w.sync(&w.a, "26.2", 4903);
    assert_eq!(
        file_of(&w.a).as_deref(),
        Some("version:4903\nkey_key.jump:key.keyboard.j\n"),
        "26.2 has no media keys: it keeps its own binding"
    );
    assert_eq!(
        shared(&w).unwrap().get("key_key.jump"),
        Some(&key("key.keyboard.media.play"))
    );
}

#[test]
fn a_mod_binding_reaches_only_an_instance_that_has_the_mod() {
    let w = world();
    put(
        &w.a,
        "version:3955\nkey_key.mymod.open:key.keyboard.o\nfov:0.0\n",
    );
    w.sync(&w.a, "1.21.1", 3955);
    put(&w.b, "version:3955\nfov:0.0\n");
    w.sync(&w.b, "1.21.1", 3955);
    assert_eq!(file_of(&w.b).as_deref(), Some("version:3955\nfov:0.0\n"));
    assert_eq!(
        shared(&w).unwrap().get("key_key.mymod.open"),
        Some(&key("key.keyboard.o"))
    );
}

/// A key the instance's file did not have at the last sync (a newer game
/// writes a new option at its default, a mod saves its bindings) seeds the
/// shared copy but never overrides it: it is a default, not an edit.
#[test]
fn a_key_new_to_an_instance_seeds_but_never_overrides() {
    let w = world();
    put(&w.a, "version:4554\ninvertXMouse:true\nfov:0.0\n");
    w.sync(&w.a, "1.21.9", 4554);
    put(&w.b, "version:4189\nfov:0.0\n");
    w.sync(&w.b, "1.21.4", 4189);
    // b moves to 1.21.9; the game writes the new option at its default.
    put(
        &w.b,
        "version:4554\ninvertXMouse:false\nfov:0.0\nsaveChatDrafts:true\n",
    );
    w.sync(&w.b, "1.21.9", 4554);
    let s = shared(&w).unwrap();
    assert_eq!(s.get("invertXMouse"), Some(&Neutral::Bool(true)));
    assert_eq!(
        s.get("saveChatDrafts"),
        Some(&Neutral::Bool(true)),
        "seeded"
    );
    assert_eq!(
        file_of(&w.b).as_deref(),
        Some("version:4554\ninvertXMouse:true\nfov:0.0\nsaveChatDrafts:true\n")
    );
    // An edit after that is an edit.
    put(
        &w.b,
        "version:4554\ninvertXMouse:false\nfov:0.0\nsaveChatDrafts:true\n",
    );
    w.sync(&w.b, "1.21.9", 4554);
    assert_eq!(
        shared(&w).unwrap().get("invertXMouse"),
        Some(&Neutral::Bool(false))
    );
}

#[test]
fn a_join_sets_the_own_file_aside_only_when_something_changes() {
    let w = world();
    put(&w.a, A_1_21_1);
    w.sync(&w.a, "1.21.1", 3955);
    put(&w.b, A_1_21_1);
    assert_eq!(w.sync(&w.b, "1.21.1", 3955), KindOutcome::Joined);
    assert!(w.set_aside.borrow().is_empty(), "nothing changed");
    let c = w.a.with_file_name("c");
    put(
        &c,
        "version:3955\nfov:0.75\nlang:en_us\nkey_key.jump:key.keyboard.space\n",
    );
    assert_eq!(w.sync(&c, "1.21.1", 3955), KindOutcome::Joined);
    assert_eq!(
        *w.set_aside.borrow(),
        ["options (before shared settings).txt"]
    );
    assert_eq!(
        std::fs::read_to_string(w.bin.join("options (before shared settings).txt")).unwrap(),
        "version:3955\nfov:0.75\nlang:en_us\nkey_key.jump:key.keyboard.space\n"
    );
    assert_eq!(
        file_of(&c).as_deref(),
        Some("version:3955\nfov:0.25\nlang:ru_ru\nkey_key.jump:key.keyboard.space\n")
    );
}

#[test]
fn make_shared_puts_this_instances_values_first_and_keeps_the_rest() {
    let w = world();
    put(&w.a, A_1_21_1);
    w.sync(&w.a, "1.21.1", 3955);
    put(&w.b, "version:3955\nfov:0.5\n");
    assert_eq!(
        w.sync_as(&w.b, &facts("1.21.1", 3955), Join::MakeShared)
            .unwrap(),
        KindOutcome::Joined
    );
    let s = shared(&w).unwrap();
    assert_eq!(s.get("fov"), Some(&Neutral::Float("0.5".into())));
    assert_eq!(s.get("lang"), Some(&Neutral::Text("ru_ru".into())));
    assert!(store::store_dir(&w.data_root)
        .join("backups")
        .join("options.json.1")
        .is_file());
    assert_eq!(file_of(&w.b).as_deref(), Some("version:3955\nfov:0.5\n"));
}

#[test]
fn a_version_the_catalog_cannot_serve_is_refused_with_its_reason() {
    let w = world();
    put(&w.a, A_1_21_1);
    let with = |data, release, mc: &str| VersionFacts {
        mc_version: mc.into(),
        data,
        release,
        loader: LoaderKind::Vanilla,
    };
    for (f, why) in [
        (
            with(DataVersion::NotRecorded, Some(true), "1.12.2"),
            Unsupported::OptionsTooOld,
        ),
        (
            with(DataVersion::Version { n: 5100 }, Some(true), "26.4"),
            Unsupported::OptionsTooNew,
        ),
        (
            with(DataVersion::NotRelease, Some(false), "26.4-pre1"),
            Unsupported::OptionsNotRelease,
        ),
        (
            with(DataVersion::JarUnavailable, None, "1.21.1"),
            Unsupported::OptionsNotInstalled,
        ),
    ] {
        assert_eq!(
            w.sync_as(&w.a, &f, Join::UseShared).unwrap(),
            KindOutcome::Unsupported { why }
        );
    }
    assert_eq!(file_of(&w.a).as_deref(), Some(A_1_21_1));
    assert!(shared(&w).is_none());
}

#[test]
fn a_1_13_instance_syncs_under_its_own_data_version() {
    let w = world();
    put(
        &w.a,
        "version:1631\nfancyGraphics:false\nkey_key.swapHands:key.keyboard.f\n",
    );
    let f = VersionFacts {
        mc_version: "1.13.2".into(),
        data: DataVersion::NotRecorded,
        release: Some(true),
        loader: LoaderKind::Vanilla,
    };
    assert_eq!(
        w.sync_as(&w.a, &f, Join::UseShared).unwrap(),
        KindOutcome::Joined
    );
    put(&w.b, "version:3955\nkey_key.swapOffhand:key.keyboard.g\n");
    w.sync(&w.b, "1.21.1", 3955);
    assert_eq!(
        file_of(&w.b).as_deref(),
        Some("version:3955\nkey_key.swapOffhand:key.keyboard.f\n"),
        "swapHands and swapOffhand are one binding"
    );
}

#[test]
fn a_write_interrupted_after_the_pending_record_is_finished_by_the_next_sync() {
    let w = world();
    put(&w.a, A_1_21_1);
    w.sync(&w.a, "1.21.1", 3955);
    put(&w.b, "version:3955\nfov:0.75\n");
    w.sync(&w.b, "1.21.1", 3955);
    put(
        &w.a,
        "version:3955\nfov:0.5\nlang:ru_ru\nkey_key.jump:key.keyboard.space\nrenderDistance:24\n",
    );
    w.sync(&w.a, "1.21.1", 3955);
    let crash = || Err(io::Error::other("power cut"));
    let seams = Seams {
        after_pending_write: &crash,
        set_aside: &|_: PathBuf| -> io::Result<()> { panic!("no set-aside on a sync") },
        ..Seams::none()
    };
    {
        let s = store::lock(&w.data_root);
        let r = sync_kind(
            &w.b,
            &s,
            &facts("1.21.1", 3955),
            Kind::Options,
            Join::Done,
            &seams,
        );
        assert!(r.is_err());
    }
    assert_eq!(
        file_of(&w.b).as_deref(),
        Some("version:3955\nfov:0.25\n"),
        "the join gave b the shared fov; the crash stopped the next write"
    );
    assert_eq!(w.sync(&w.b, "1.21.1", 3955), KindOutcome::Synced);
    assert_eq!(file_of(&w.b).as_deref(), Some("version:3955\nfov:0.5\n"));
}

#[test]
fn a_damaged_shared_copy_is_an_error_that_names_it() {
    let w = world();
    put(&w.a, A_1_21_1);
    let dir = store::store_dir(&w.data_root);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("options.json"), b"{not json").unwrap();
    let err = w
        .sync_as(&w.a, &facts("1.21.1", 3955), Join::UseShared)
        .err()
        .expect("an error");
    assert_eq!(
        classify(&err),
        (Some(SyncFile::Shared), FileReason::Damaged)
    );
    assert_eq!(file_of(&w.a).as_deref(), Some(A_1_21_1));
}

// --- When the records no longer describe the file (review round, spec §15) ---

fn sync_done(w: &World, root: &Path, mc: &str, dv: i32) -> crate::error::Result<KindOutcome> {
    w.sync_as(root, &facts(mc, dv), Join::Done)
}

fn shared_fov(w: &World) -> Option<Neutral> {
    shared(w).unwrap().get("fov").cloned()
}

/// Deleting options.txt is a common reset: the game then writes its defaults,
/// which must never reach the other instances as edits.
#[test]
fn a_deleted_file_rejoins_and_the_games_defaults_never_spread() {
    let w = world();
    put(&w.a, A_1_21_1);
    w.sync(&w.a, "1.21.1", 3955);
    std::fs::remove_file(game_file(&w.a, Kind::Options)).unwrap();
    assert_eq!(
        sync_done(&w, &w.a, "1.21.1", 3955).unwrap(),
        KindOutcome::Waiting {
            why: WaitReason::FirstLaunch
        }
    );
    put(
        &w.a,
        "version:3955\nfov:0.0\nlang:en_us\nkey_key.jump:key.keyboard.space\n",
    );
    sync_done(&w, &w.a, "1.21.1", 3955).unwrap();
    assert_eq!(shared_fov(&w), Some(Neutral::Float("0.25".into())));
    assert_eq!(
        file_of(&w.a).as_deref(),
        Some("version:3955\nfov:0.25\nlang:ru_ru\nkey_key.jump:key.keyboard.space\n"),
        "the shared settings came back"
    );
}

#[test]
fn a_packs_unversioned_copy_rejoins_once_the_game_saves_it() {
    let w = world();
    put(&w.a, A_1_21_1);
    w.sync(&w.a, "1.21.1", 3955);
    put(&w.a, "fov:0.9\nlang:de_de\n");
    sync_done(&w, &w.a, "1.21.1", 3955).unwrap();
    put(&w.a, "version:3955\nfov:0.9\nlang:de_de\n");
    sync_done(&w, &w.a, "1.21.1", 3955).unwrap();
    assert_eq!(shared_fov(&w), Some(Neutral::Float("0.25".into())));
}

/// An older game drops what it cannot name and saves its defaults instead.
#[test]
fn a_file_an_older_version_saved_rejoins() {
    let w = world();
    put(&w.a, "version:3105\nmainHand:\"left\"\nfov:0.25\n");
    w.sync(&w.a, "1.19", 3105);
    put(&w.a, "version:2975\nmainHand:right\nfov:0.25\n");
    sync_done(&w, &w.a, "1.18.2", 2975).unwrap();
    assert_eq!(
        shared(&w).unwrap().get("mainHand"),
        Some(&Neutral::Text("left".into()))
    );
    assert_eq!(
        file_of(&w.a).as_deref(),
        Some("version:2975\nmainHand:left\nfov:0.25\n")
    );
}

/// After an upgrade the old file waits for the new game; what the user then
/// changes in the new game is an edit.
#[test]
fn an_upgrade_still_captures_what_changed() {
    let w = world();
    put(&w.a, "version:4189\nlang:ru_ru\nfov:0.25\n");
    w.sync(&w.a, "1.21.4", 4189);
    assert_eq!(
        sync_done(&w, &w.a, "1.21.9", 4554).unwrap(),
        KindOutcome::Waiting {
            why: WaitReason::GameSave
        }
    );
    put(&w.a, "version:4554\nlang:de_de\nfov:0.25\n");
    sync_done(&w, &w.a, "1.21.9", 4554).unwrap();
    assert_eq!(
        shared(&w).unwrap().get("lang"),
        Some(&Neutral::Text("de_de".into()))
    );
}

/// A file the game could not read is replaced by its defaults on a later
/// save; those are no edits either.
#[test]
fn a_damaged_file_rejoins_once_the_game_rewrites_it() {
    let w = world();
    put(&w.a, A_1_21_1);
    w.sync(&w.a, "1.21.1", 3955);
    std::fs::write(game_file(&w.a, Kind::Options), b"version:3955\nlang:\xff\n").unwrap();
    assert!(sync_done(&w, &w.a, "1.21.1", 3955).is_err());
    put(&w.a, "version:3955\nfov:0.0\nlang:en_us\n");
    sync_done(&w, &w.a, "1.21.1", 3955).unwrap();
    assert_eq!(shared_fov(&w), Some(Neutral::Float("0.25".into())));
}

/// Before 1.19 sound volumes are Java floats: an older game re-spells the
/// shared double, which is the same value, not an edit.
#[test]
fn an_older_game_re_spelling_a_float_is_no_edit() {
    let w = world();
    put(
        &w.a,
        "version:3105\nsoundCategory_master:0.43661971830985913\n",
    );
    w.sync(&w.a, "1.19", 3105);
    put(&w.b, "version:2975\nsoundCategory_master:1.0\n");
    w.sync(&w.b, "1.18.2", 2975);
    assert_eq!(
        file_of(&w.b).as_deref(),
        Some("version:2975\nsoundCategory_master:0.43661971830985913\n")
    );
    put(&w.b, "version:2975\nsoundCategory_master:0.43661973\n");
    put(&w.a, "version:3105\nsoundCategory_master:0.5\n");
    sync_done(&w, &w.a, "1.19", 3105).unwrap();
    sync_done(&w, &w.b, "1.18.2", 2975).unwrap();
    assert_eq!(
        shared(&w).unwrap().get("soundCategory_master"),
        Some(&Neutral::Float("0.5".into())),
        "a's edit stands"
    );
}

/// A value this game can only say lossily (fabulous on a two-level game) is
/// written as the nearest it has, and is not captured back.
#[test]
fn a_lossy_value_does_not_come_back_as_an_edit() {
    let w = world();
    put(&w.a, "version:3955\ngraphicsMode:2\n");
    w.sync(&w.a, "1.21.1", 3955);
    let old = VersionFacts {
        mc_version: "1.13.2".into(),
        data: DataVersion::NotRecorded,
        release: Some(true),
        loader: LoaderKind::Vanilla,
    };
    put(&w.b, "version:1631\nfancyGraphics:false\n");
    w.sync_as(&w.b, &old, Join::UseShared).unwrap();
    assert_eq!(
        file_of(&w.b).as_deref(),
        Some("version:1631\nfancyGraphics:true\n")
    );
    w.sync_as(&w.b, &old, Join::Done).unwrap();
    w.sync_as(&w.b, &old, Join::Done).unwrap();
    assert_eq!(
        shared(&w).unwrap().get("graphics"),
        Some(&Neutral::Text("fabulous".into()))
    );
}

/// Forge reads a key modifier; a loader without modifiers keeps its own
/// binding rather than a bare key that would clash with another.
#[test]
fn a_modifier_binding_reaches_forge_and_no_bare_key_elsewhere() {
    let w = world();
    let forge = |mc: &str, dv| VersionFacts {
        loader: LoaderKind::Forge,
        ..facts(mc, dv)
    };
    put(
        &w.a,
        "version:3465\nkey_key.mymod.open:key.keyboard.o:SHIFT\n",
    );
    w.sync_as(&w.a, &forge("1.20.1", 3465), Join::UseShared)
        .unwrap();
    put(&w.b, "version:3465\nkey_key.mymod.open:key.keyboard.p\n");
    w.sync(&w.b, "1.20.1", 3465);
    assert_eq!(
        file_of(&w.b).as_deref(),
        Some("version:3465\nkey_key.mymod.open:key.keyboard.p\n")
    );
    let c = w.a.with_file_name("c");
    put(&c, "version:3465\nkey_key.mymod.open:key.keyboard.p\n");
    w.sync_as(&c, &forge("1.20.1", 3465), Join::UseShared)
        .unwrap();
    assert_eq!(
        file_of(&c).as_deref(),
        Some("version:3465\nkey_key.mymod.open:key.keyboard.o:SHIFT\n")
    );
}

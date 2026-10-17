//! The sync point (spec §B2) for one instance and one kind, under the store
//! lock, with the instance not running:
//!
//! 1. resolve a pending write left by a crash;
//! 2. capture what changed here since the baseline into the shared copy —
//!    across a Minecraft version change too (corrected while reviewing PR 2:
//!    a "version step" that re-baselined without capturing dropped every
//!    hotbar save of a newer game, and any edit an exit sync had missed).
//!    Hotbars are captured under the DataVersion of the game that wrote the
//!    file, so a newer game's save raises the shared version (forward only);
//! 3. apply the shared copy back: pending → the instance file → baseline —
//!    except hotbars newer than this game, which it could not read.
//!
//! No baseline yet is a join: servers and history are united with the shared
//! copy (nothing lost), hotbars take the shared rows (the instance's own go to
//! the Recycle Bin first) or give theirs.
//! Every write is atomic; [`Seams`] lets a test fail right after each one.

use std::io;
use std::path::{Path, PathBuf};

use fastnbt::Value;
use serde::Serialize;

use super::baseline::{self, Slot, Snapshot};
use super::failure::{self, At};
use super::history::{self, Sep};
use super::hotbars::{self, Hotbars};
use super::servers::{self, Entry};
use super::sidecar::Join;
use super::store::StoreLock;
use super::version::{DataVersion, VersionFacts};
use super::{game_file, Kind};
use crate::datapacks::level_dat::{self, Framing};
use crate::error::{Error, Result};

/// A hook run right after one of the sync point's writes.
pub type Seam<'a> = &'a dyn Fn() -> io::Result<()>;

/// Fault injection for tests, and where a join puts the hotbars it replaces;
/// production uses [`Seams::none`]: no faults, the Recycle Bin.
pub struct Seams<'a> {
    pub after_capture_save: Seam<'a>,
    pub after_pending_write: Seam<'a>,
    pub after_instance_write: Seam<'a>,
    /// Takes away a copy of the instance's own hotbars that a join is about
    /// to replace (spec §B7): the Recycle Bin, under a recognisable name.
    pub set_aside: &'a dyn Fn(PathBuf) -> io::Result<()>,
}

fn no_fault() -> io::Result<()> {
    Ok(())
}

#[cfg(not(test))]
fn to_recycle_bin(path: PathBuf) -> io::Result<()> {
    crate::platform::recycle_bin::to_recycle_bin(path)
}

/// A test never reaches the user's Recycle Bin: one that needs a set-aside
/// passes its own.
#[cfg(test)]
fn to_recycle_bin(path: PathBuf) -> io::Result<()> {
    panic!(
        "a test reached the real Recycle Bin with {}",
        path.display()
    )
}

impl Seams<'static> {
    pub fn none() -> Self {
        Seams {
            after_capture_save: &no_fault,
            after_pending_write: &no_fault,
            after_instance_write: &no_fault,
            set_aside: &to_recycle_bin,
        }
    }
}

/// How one kind's sync point ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KindOutcome {
    Synced,
    /// A join completed.
    Joined,
    Unsupported {
        why: Unsupported,
    },
    /// Shared hotbars were saved by a newer Minecraft: this instance keeps its own.
    OlderThanShared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Unsupported {
    /// command_history.txt exists from 1.20.2.
    HistoryTooOld,
    /// The game records no DataVersion (before 1.14).
    HotbarsNotRecorded,
    /// The game files are not installed yet (or the jar is damaged).
    HotbarsNotInstalled,
    /// A snapshot or pre-release.
    HotbarsNotRelease,
}

pub fn sync_kind(
    instance_root: &Path,
    store: &StoreLock,
    facts: &VersionFacts,
    kind: Kind,
    join: Join,
    seams: &Seams,
) -> Result<KindOutcome> {
    if let Some(why) = unsupported(kind, facts) {
        return Ok(KindOutcome::Unsupported { why });
    }
    let path = game_file(instance_root, kind);
    resolve_pending(instance_root, kind, &path)?;

    let now = read_doc(kind, &path)?;
    let shared = match store.load(kind)? {
        Some(bytes) => Some(parse_doc(kind, &bytes).map_err(|why| {
            failure::damaged(At::shared(kind), &store.dir().join(kind.file()), why)
        })?),
        None => None,
    };
    let base = baseline::read(instance_root, kind, Slot::Baseline)?;

    if kind == Kind::Hotbars && join != Join::MakeShared {
        if let (Some(Doc::Hotbars(s)), Some(dv)) = (&shared, facts.hotbar_version()) {
            if s.data_version.is_some_and(|shared_dv| shared_dv > dv) {
                return Ok(KindOutcome::OlderThanShared);
            }
        }
    }

    let (merged, outcome) = match &base {
        None => (
            joined(kind, join, now.as_ref(), shared.as_ref()),
            KindOutcome::Joined,
        ),
        Some(b) => {
            let before = doc_from_snapshot(kind, b).map_err(|why| {
                failure::damaged(
                    At::records(Some(kind)),
                    &baseline::path(instance_root, kind, Slot::Baseline),
                    why,
                )
            })?;
            let merged = match (&now, &shared) {
                // A missing file is "nothing saved here", never "emptied".
                (None, Some(s)) => s.clone(),
                (None, None) => Doc::empty(kind),
                (Some(n), None) => n.clone(),
                (Some(n), Some(s)) => capture(&before, n, s, facts),
            };
            (merged, KindOutcome::Synced)
        }
    };

    let changed = match &shared {
        Some(s) => !s.same(&merged),
        None => !merged.is_empty(),
    };
    if changed {
        store.save(kind, &merged.bytes()?)?;
        (seams.after_capture_save)().map_err(|e| Error::io("<after capture>", e))?;
    }

    // Hotbars this game cannot read (a newer one wrote them here, and the
    // capture just lifted the shared copy above this game): it keeps its own
    // file, which is what it already had.
    if too_new_for(&merged, facts) {
        let current = now.as_ref().unwrap_or(&merged);
        baseline::write(
            instance_root,
            kind,
            Slot::Baseline,
            &snapshot(facts, current),
        )?;
        return Ok(outcome);
    }

    let next = render(now.as_ref(), &merged, facts);
    let differs = match &now {
        Some(n) => !n.same(&next),
        None => !next.is_empty(),
    };
    if differs {
        // A join that replaces this instance's own hotbars keeps them first.
        if outcome == KindOutcome::Joined && kind == Kind::Hotbars && now.is_some() {
            set_aside_own_hotbars(&path, seams)?;
        }
        let snap = snapshot(facts, &next);
        baseline::write(instance_root, kind, Slot::Pending, &snap)?;
        (seams.after_pending_write)().map_err(|e| Error::io("<after pending>", e))?;
        super::fsio::write_atomic(At::game(kind), &path, &next.bytes()?)?;
        (seams.after_instance_write)().map_err(|e| Error::io("<after instance>", e))?;
        baseline::write(instance_root, kind, Slot::Baseline, &snap)?;
        baseline::remove(instance_root, kind, Slot::Pending)?;
    } else {
        let current = now.as_ref().unwrap_or(&next);
        baseline::write(
            instance_root,
            kind,
            Slot::Baseline,
            &snapshot(facts, current),
        )?;
    }
    Ok(outcome)
}

pub(crate) fn unsupported(kind: Kind, facts: &VersionFacts) -> Option<Unsupported> {
    match kind {
        Kind::Servers => None,
        Kind::History => (!facts.history_supported()).then_some(Unsupported::HistoryTooOld),
        Kind::Hotbars => match facts.data {
            DataVersion::Version { .. } => None,
            DataVersion::NotRecorded => Some(Unsupported::HotbarsNotRecorded),
            DataVersion::JarUnavailable => Some(Unsupported::HotbarsNotInstalled),
            DataVersion::NotRelease => Some(Unsupported::HotbarsNotRelease),
        },
    }
}

/// The name the instance's own hotbars get in the Recycle Bin when a join
/// replaces them.
const SET_ASIDE_NAME: &str = "hotbar (before shared hotbars).nbt";

/// A copy of the instance's own hotbars goes to the Recycle Bin before a
/// join replaces them (spec §B7). If that fails the join stops here and
/// nothing has changed: the copy is removed (checked) and the instance's
/// file is untouched.
fn set_aside_own_hotbars(path: &Path, seams: &Seams) -> Result<()> {
    let at = At::game(Kind::Hotbars);
    let Some(bytes) = super::fsio::read_if_present(at, path)? else {
        return Ok(());
    };
    let aside = path.with_file_name(SET_ASIDE_NAME);
    super::fsio::write_atomic(at, &aside, &bytes)?;
    if let Err(e) = (seams.set_aside)(aside.clone()) {
        if let Err(r) = super::fsio::remove_if_present(at, &aside) {
            crate::diag!(
                "settings sync: could not remove the hotbars copy {} ({r})",
                aside.display()
            );
        }
        return Err(failure::io(at, &aside, e));
    }
    Ok(())
}

/// Whether `doc` is hotbars of a newer DataVersion than this game's.
fn too_new_for(doc: &Doc, facts: &VersionFacts) -> bool {
    match (doc, facts.hotbar_version()) {
        (Doc::Hotbars(h), Some(mine)) => h.data_version.is_some_and(|dv| dv > mine),
        _ => false,
    }
}

/// Step 1: a pending file whose content landed becomes the baseline; one
/// whose write never landed is dropped. Its removal is checked.
fn resolve_pending(instance_root: &Path, kind: Kind, path: &Path) -> Result<()> {
    let Some(pending) = baseline::read(instance_root, kind, Slot::Pending)? else {
        return Ok(());
    };
    let now = read_doc(kind, path)?;
    if now.is_some_and(|n| n.data() == pending.data && n.stamp() == pending.stamp) {
        baseline::write(instance_root, kind, Slot::Baseline, &pending)?;
    }
    baseline::remove(instance_root, kind, Slot::Pending)
}

/// A join (no baseline yet): seed an empty store, unite servers and history,
/// and for hotbars take the shared rows or give this instance's.
fn joined(kind: Kind, join: Join, now: Option<&Doc>, shared: Option<&Doc>) -> Doc {
    match (shared, now) {
        (None, Some(n)) => n.clone(),
        (None, None) => Doc::empty(kind),
        (Some(s), None) => s.clone(),
        (Some(s), Some(n)) => match (s, n) {
            (Doc::Servers(s), Doc::Servers(n)) => Doc::Servers(servers::union(s, n)),
            (Doc::History(s, sep), Doc::History(n, _)) => {
                Doc::History(history::capture(&[], n, s), *sep)
            }
            (Doc::Hotbars(_), Doc::Hotbars(_)) if join == Join::MakeShared => n.clone(),
            _ => s.clone(),
        },
    }
}

fn capture(before: &Doc, now: &Doc, shared: &Doc, facts: &VersionFacts) -> Doc {
    match (before, now, shared) {
        (Doc::Servers(b), Doc::Servers(n), Doc::Servers(s)) => {
            Doc::Servers(servers::capture(b, n, s))
        }
        (Doc::History(b, _), Doc::History(n, _), Doc::History(s, sep)) => {
            Doc::History(history::capture(b, n, s), *sep)
        }
        (Doc::Hotbars(b), Doc::Hotbars(n), Doc::Hotbars(s)) => {
            // The version of the game that wrote the file, not this one's: a
            // file a newer game saved before the instance moved back is
            // stamped as that newer game's.
            let dv = n.data_version.or(facts.hotbar_version()).unwrap_or(0);
            Doc::Hotbars(hotbars::capture(b, n, s, dv))
        }
        // Kinds never mix: every Doc here was read for one `kind`.
        _ => shared.clone(),
    }
}

/// Step 4's target: the shared copy, in this instance's form.
fn render(now: Option<&Doc>, shared: &Doc, facts: &VersionFacts) -> Doc {
    match shared {
        Doc::Servers(s) => Doc::Servers(servers::render(s, facts.shows_hidden_servers())),
        Doc::History(lines, sep) => {
            let sep = match now {
                Some(Doc::History(_, mine)) => *mine,
                _ => *sep,
            };
            Doc::History(lines.clone(), sep)
        }
        Doc::Hotbars(h) => {
            let framing = match now {
                Some(Doc::Hotbars(mine)) => mine.framing,
                _ => h.framing,
            };
            Doc::Hotbars(Hotbars {
                framing,
                ..h.clone()
            })
        }
    }
}

fn snapshot(facts: &VersionFacts, doc: &Doc) -> Snapshot {
    Snapshot {
        mc: facts.mc_version.clone(),
        stamp: doc.stamp(),
        data: doc.data(),
    }
}

/// One kind's file, parsed.
#[derive(Debug, Clone)]
enum Doc {
    Servers(Vec<Entry>),
    History(Vec<String>, Sep),
    Hotbars(Hotbars),
}

impl Doc {
    fn empty(kind: Kind) -> Doc {
        match kind {
            Kind::Servers => Doc::Servers(Vec::new()),
            Kind::History => Doc::History(Vec::new(), Sep::platform()),
            Kind::Hotbars => Doc::Hotbars(Hotbars {
                rows: Default::default(),
                data_version: None,
                framing: Framing::Raw,
            }),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Doc::Servers(s) => s.is_empty(),
            Doc::History(l, _) => l.is_empty(),
            Doc::Hotbars(h) => h.rows.is_empty(),
        }
    }

    /// The projection the merge compares (never raw bytes).
    fn data(&self) -> Value {
        match self {
            Doc::Servers(s) => servers::to_value(s),
            Doc::History(l, _) => Value::List(l.iter().cloned().map(Value::String).collect()),
            Doc::Hotbars(h) => hotbars::rows_value(h),
        }
    }

    fn stamp(&self) -> Option<i32> {
        match self {
            Doc::Hotbars(h) => h.data_version,
            _ => None,
        }
    }

    fn same(&self, other: &Doc) -> bool {
        self.data() == other.data() && self.stamp() == other.stamp()
    }

    fn bytes(&self) -> Result<Vec<u8>> {
        match self {
            Doc::Servers(s) => servers::to_bytes(s),
            Doc::History(l, sep) => Ok(history::render(l, *sep)),
            Doc::Hotbars(h) => hotbars::to_bytes(h),
        }
    }
}

fn read_doc(kind: Kind, path: &Path) -> Result<Option<Doc>> {
    Ok(match kind {
        Kind::Servers => servers::read(path)?.map(Doc::Servers),
        Kind::History => history::read(path)?.map(|(l, sep)| Doc::History(l, sep)),
        Kind::Hotbars => hotbars::read(path)?.map(Doc::Hotbars),
    })
}

fn parse_doc(kind: Kind, bytes: &[u8]) -> std::result::Result<Doc, String> {
    match kind {
        Kind::Servers => servers::parse(bytes).map(Doc::Servers),
        Kind::History => {
            let text = std::str::from_utf8(bytes).map_err(|e| format!("not UTF-8: {e}"))?;
            let (lines, sep) = history::split(text);
            Ok(Doc::History(lines, sep))
        }
        Kind::Hotbars => {
            let (root, framing) = level_dat::parse(bytes).map_err(|e| e.to_string())?;
            hotbars::from_root(root, framing).map(Doc::Hotbars)
        }
    }
}

fn doc_from_snapshot(kind: Kind, snap: &Snapshot) -> std::result::Result<Doc, String> {
    match kind {
        Kind::Servers => servers::from_value(snap.data.clone()).map(Doc::Servers),
        Kind::History => match &snap.data {
            Value::List(items) => items
                .iter()
                .map(|v| match v {
                    Value::String(s) => Ok(s.clone()),
                    _ => Err("a history line is not a string".to_string()),
                })
                .collect::<std::result::Result<Vec<_>, _>>()
                .map(|l| Doc::History(l, Sep::platform())),
            _ => Err("the history is not a list".into()),
        },
        Kind::Hotbars => match &snap.data {
            Value::Compound(rows) => Ok(Doc::Hotbars(Hotbars {
                rows: rows.clone().into_iter().collect(),
                data_version: snap.stamp,
                framing: Framing::Raw,
            })),
            _ => Err("the hotbars are not a compound".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;
    use crate::settings_sync::store;

    struct World {
        _td: tempfile::TempDir,
        data_root: PathBuf,
        a: PathBuf,
        b: PathBuf,
    }

    fn world() -> World {
        let td = tempfile::tempdir().unwrap();
        let data_root = td.path().join("root");
        let a = td.path().join("a");
        let b = td.path().join("b");
        World {
            _td: td,
            data_root,
            a,
            b,
        }
    }

    fn facts(mc: &str) -> VersionFacts {
        VersionFacts {
            mc_version: mc.into(),
            data: DataVersion::Version { n: 3955 },
            release: Some(true),
            loader: crate::instances::schema::LoaderKind::Vanilla,
        }
    }

    fn entry(name: &str, ip: &str) -> Entry {
        let mut m = HashMap::new();
        m.insert("name".into(), Value::String(name.into()));
        m.insert("ip".into(), Value::String(ip.into()));
        m
    }

    fn put_servers(root: &Path, names: &[&str]) {
        let list: Vec<Entry> = names.iter().map(|n| entry(n, &n.to_lowercase())).collect();
        let p = game_file(root, Kind::Servers);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, servers::to_bytes(&list).unwrap()).unwrap();
    }

    fn servers_of(root: &Path) -> Vec<String> {
        servers::read(&game_file(root, Kind::Servers))
            .unwrap()
            .unwrap_or_default()
            .iter()
            .map(|e| match e.get("name") {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            })
            .collect()
    }

    fn shared_servers(w: &World) -> Vec<String> {
        let s = store::lock(&w.data_root);
        let Some(bytes) = s.load(Kind::Servers).unwrap() else {
            return Vec::new();
        };
        servers::parse(&bytes)
            .unwrap()
            .iter()
            .map(|e| match e.get("name") {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            })
            .collect()
    }

    fn sync(w: &World, root: &Path, kind: Kind, mc: &str) -> Result<KindOutcome> {
        let s = store::lock(&w.data_root);
        sync_kind(root, &s, &facts(mc), kind, Join::UseShared, &Seams::none())
    }

    #[test]
    fn the_first_member_seeds_the_shared_list_and_keeps_its_file() {
        let w = world();
        put_servers(&w.a, &["A", "B"]);
        assert_eq!(
            sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap(),
            KindOutcome::Joined
        );
        assert_eq!(shared_servers(&w), ["A", "B"]);
        assert_eq!(servers_of(&w.a), ["A", "B"]);
    }

    #[test]
    fn a_second_member_joins_by_union() {
        let w = world();
        put_servers(&w.a, &["A", "B"]);
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        put_servers(&w.b, &["B", "M"]);
        sync(&w, &w.b, Kind::Servers, "1.21.1").unwrap();
        assert_eq!(shared_servers(&w), ["A", "B", "M"]);
        assert_eq!(servers_of(&w.b), ["A", "B", "M"]);
    }

    #[test]
    fn two_instances_changing_different_things_keep_both() {
        let w = world();
        put_servers(&w.a, &["A"]);
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        sync(&w, &w.b, Kind::Servers, "1.21.1").unwrap();
        // Both games run and each adds a server.
        put_servers(&w.a, &["A", "X"]);
        put_servers(&w.b, &["A", "Y"]);
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        sync(&w, &w.b, Kind::Servers, "1.21.1").unwrap();
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        // Y was captured after X and lands after its neighbour A (spec §B4).
        assert_eq!(servers_of(&w.a), ["A", "Y", "X"]);
        assert_eq!(servers_of(&w.b), ["A", "Y", "X"]);
    }

    #[test]
    fn a_stale_instance_does_not_undo_a_change_made_elsewhere() {
        let w = world();
        put_servers(&w.a, &["A", "B"]);
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        sync(&w, &w.b, Kind::Servers, "1.21.1").unwrap();
        put_servers(&w.a, &["A"]); // B removed in a
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        sync(&w, &w.b, Kind::Servers, "1.21.1").unwrap(); // b still lists B, unchanged here
        assert_eq!(servers_of(&w.b), ["A"]);
        assert_eq!(shared_servers(&w), ["A"]);
    }

    #[test]
    fn a_missing_file_captures_nothing_and_gets_the_shared_list() {
        let w = world();
        put_servers(&w.a, &["A", "B"]);
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        std::fs::remove_file(game_file(&w.a, Kind::Servers)).unwrap();
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        assert_eq!(shared_servers(&w), ["A", "B"]);
        assert_eq!(servers_of(&w.a), ["A", "B"]);
    }

    #[test]
    fn a_change_made_before_a_version_change_is_still_captured() {
        // The exit sync failed (or was skipped), then the instance moved to a
        // new Minecraft version: the removal made in that session still
        // reaches the shared list. Servers have no DataFixer to fear.
        let w = world();
        put_servers(&w.a, &["A", "B"]);
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        put_servers(&w.a, &["A"]);
        assert_eq!(
            sync(&w, &w.a, Kind::Servers, "1.21.4").unwrap(),
            KindOutcome::Synced
        );
        assert_eq!(shared_servers(&w), ["A"]);
    }

    fn facts_dv(mc: &str, n: i32) -> VersionFacts {
        VersionFacts {
            mc_version: mc.into(),
            data: DataVersion::Version { n },
            release: Some(true),
            loader: crate::instances::schema::LoaderKind::Vanilla,
        }
    }

    fn slot(item: &str) -> Value {
        let mut m = HashMap::new();
        m.insert("id".to_string(), Value::String(item.into()));
        m.insert("count".to_string(), Value::Int(1));
        Value::List(vec![Value::Compound(m)])
    }

    fn bars(rows: &[(&str, &str)], dv: i32) -> Hotbars {
        Hotbars {
            rows: rows.iter().map(|(k, v)| (k.to_string(), slot(v))).collect(),
            data_version: Some(dv),
            framing: Framing::Raw,
        }
    }

    fn put_bars(root: &Path, h: &Hotbars) -> Vec<u8> {
        let p = game_file(root, Kind::Hotbars);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let bytes = hotbars::to_bytes(h).unwrap();
        std::fs::write(p, &bytes).unwrap();
        bytes
    }

    fn bars_of(root: &Path) -> Hotbars {
        hotbars::read(&game_file(root, Kind::Hotbars))
            .unwrap()
            .unwrap()
    }

    fn shared_bars(w: &World) -> Hotbars {
        let bytes = store::lock(&w.data_root)
            .load(Kind::Hotbars)
            .unwrap()
            .unwrap();
        let (root, framing) = level_dat::parse(&bytes).unwrap();
        hotbars::from_root(root, framing).unwrap()
    }

    fn sync_bars(
        w: &World,
        root: &Path,
        f: &VersionFacts,
        join: Join,
        seams: &Seams,
    ) -> KindOutcome {
        let s = store::lock(&w.data_root);
        sync_kind(root, &s, f, Kind::Hotbars, join, seams).unwrap()
    }

    /// Seams that record what a join set aside instead of using the Recycle Bin.
    fn recording(
        aside: &std::cell::RefCell<Vec<(PathBuf, Vec<u8>)>>,
    ) -> impl Fn(PathBuf) -> io::Result<()> + '_ {
        move |p: PathBuf| {
            aside.borrow_mut().push((p.clone(), std::fs::read(&p)?));
            std::fs::remove_file(&p)
        }
    }

    #[test]
    fn a_newer_game_saving_the_shared_hotbars_is_captured_and_raises_the_shared_version() {
        let w = world();
        put_bars(&w.a, &bars(&[("0", "stone")], 3955));
        let (old, new) = (facts_dv("1.21.1", 3955), facts_dv("1.21.4", 4189));
        sync_bars(&w, &w.a, &old, Join::UseShared, &Seams::none());
        // b (a newer game) takes the shared rows, then the user saves a hotbar.
        sync_bars(&w, &w.b, &new, Join::UseShared, &Seams::none());
        put_bars(&w.b, &bars(&[("0", "diamond")], 4189));
        assert_eq!(
            sync_bars(&w, &w.b, &new, Join::Done, &Seams::none()),
            KindOutcome::Synced
        );
        let shared = shared_bars(&w);
        assert_eq!(shared.rows["0"], slot("diamond"), "the save was captured");
        assert_eq!(shared.data_version, Some(4189));
        assert_eq!(bars_of(&w.b).rows["0"], slot("diamond"), "and kept");
    }

    #[test]
    fn hotbars_a_newer_game_wrote_are_captured_but_not_written_into_an_older_one() {
        // The instance went back to 1.21.1 after a 1.21.4 game saved hotbars
        // its exit sync did not capture: they reach the shared copy under the
        // version that wrote them, and the 1.21.1 game is not handed rows it
        // cannot read.
        let w = world();
        let old = facts_dv("1.21.1", 3955);
        put_bars(&w.a, &bars(&[("0", "stone")], 3955));
        sync_bars(&w, &w.a, &old, Join::UseShared, &Seams::none());
        let newer = put_bars(&w.a, &bars(&[("0", "glass")], 4189));
        sync_bars(&w, &w.a, &old, Join::Done, &Seams::none());
        let shared = shared_bars(&w);
        assert_eq!(shared.rows["0"], slot("glass"));
        assert_eq!(shared.data_version, Some(4189));
        assert_eq!(
            std::fs::read(game_file(&w.a, Kind::Hotbars)).unwrap(),
            newer,
            "untouched"
        );
    }

    #[test]
    fn joining_with_the_shared_hotbars_sets_this_instances_own_aside() {
        let w = world();
        let f = facts_dv("1.21.1", 3955);
        put_bars(&w.a, &bars(&[("0", "stone")], 3955));
        sync_bars(&w, &w.a, &f, Join::UseShared, &Seams::none());
        let mine = put_bars(&w.b, &bars(&[("0", "dirt")], 3955));
        let aside = std::cell::RefCell::new(Vec::new());
        let set_aside = recording(&aside);
        let seams = Seams {
            set_aside: &set_aside,
            ..Seams::none()
        };
        sync_bars(&w, &w.b, &f, Join::UseShared, &seams);
        assert_eq!(bars_of(&w.b).rows["0"], slot("stone"), "the shared rows");
        let aside = aside.borrow();
        assert_eq!(aside.len(), 1);
        assert_eq!(
            aside[0].0.file_name().unwrap(),
            "hotbar (before shared hotbars).nbt"
        );
        assert_eq!(aside[0].1, mine, "its own hotbars, whole");
    }

    #[test]
    fn a_join_whose_hotbars_cannot_be_set_aside_changes_nothing() {
        let w = world();
        let f = facts_dv("1.21.1", 3955);
        put_bars(&w.a, &bars(&[("0", "stone")], 3955));
        sync_bars(&w, &w.a, &f, Join::UseShared, &Seams::none());
        let mine = put_bars(&w.b, &bars(&[("0", "dirt")], 3955));
        let refuse = |_: PathBuf| -> io::Result<()> { Err(io::Error::other("no bin")) };
        let seams = Seams {
            set_aside: &refuse,
            ..Seams::none()
        };
        let s = store::lock(&w.data_root);
        assert!(sync_kind(&w.b, &s, &f, Kind::Hotbars, Join::UseShared, &seams).is_err());
        assert_eq!(std::fs::read(game_file(&w.b, Kind::Hotbars)).unwrap(), mine);
        let left: Vec<_> = std::fs::read_dir(game_file(&w.b, Kind::Hotbars).parent().unwrap())
            .unwrap()
            .collect();
        assert_eq!(left.len(), 1, "no copy left beside it");
    }

    #[test]
    fn an_unreadable_instance_file_is_an_error_and_nothing_moves() {
        let w = world();
        put_servers(&w.a, &["A"]);
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        std::fs::write(game_file(&w.a, Kind::Servers), b"garbage").unwrap();
        assert!(sync(&w, &w.a, Kind::Servers, "1.21.1").is_err());
        assert_eq!(shared_servers(&w), ["A"]);
    }

    /// Fail right after one write, then run the sync point again (spec §B2's
    /// crash table): the result is as if nothing had failed.
    fn crash_after(which: usize) {
        let w = world();
        put_servers(&w.a, &["A"]);
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        put_servers(&w.b, &["A"]);
        sync(&w, &w.b, Kind::Servers, "1.21.1").unwrap();
        // b adds Y; a adds X and then crashes at `which`.
        put_servers(&w.b, &["A", "Y"]);
        sync(&w, &w.b, Kind::Servers, "1.21.1").unwrap();
        put_servers(&w.a, &["A", "X"]);
        let hit = Cell::new(false);
        let fail = || {
            hit.set(true);
            Err(io::Error::other("power cut"))
        };
        let seams = Seams {
            after_capture_save: if which == 0 { &fail } else { &no_fault },
            after_pending_write: if which == 1 { &fail } else { &no_fault },
            after_instance_write: if which == 2 { &fail } else { &no_fault },
            set_aside: &to_recycle_bin,
        };
        {
            let s = store::lock(&w.data_root);
            let got = sync_kind(
                &w.a,
                &s,
                &facts("1.21.1"),
                Kind::Servers,
                Join::UseShared,
                &seams,
            );
            assert!(got.is_err(), "the injected failure stops the sync point");
        }
        assert!(hit.get(), "the seam was reached");
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        // X, captured after Y, lands after its neighbour A (spec §B4).
        assert_eq!(
            shared_servers(&w),
            ["A", "X", "Y"],
            "crash after write {which}"
        );
        assert_eq!(
            servers_of(&w.a),
            ["A", "X", "Y"],
            "crash after write {which}"
        );
        assert!(
            baseline::read(&w.a, Kind::Servers, Slot::Pending)
                .unwrap()
                .is_none(),
            "no pending file left"
        );
    }

    #[test]
    fn a_crash_after_saving_the_capture_recovers() {
        crash_after(0);
    }

    #[test]
    fn a_crash_after_writing_pending_recovers() {
        crash_after(1);
    }

    #[test]
    fn a_crash_after_writing_the_instance_file_recovers() {
        crash_after(2);
    }

    /// A crash after the instance write leaves the pending file equal to the
    /// file: it becomes the baseline, so what was applied is not captured
    /// back as a change here — a server removed elsewhere meanwhile stays
    /// removed.
    #[test]
    fn a_landed_pending_write_becomes_the_baseline() {
        let w = world();
        put_servers(&w.a, &["A", "Y"]);
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        put_servers(&w.b, &["A", "Y"]);
        sync(&w, &w.b, Kind::Servers, "1.21.1").unwrap();
        put_servers(&w.b, &["A", "Y", "Z"]);
        sync(&w, &w.b, Kind::Servers, "1.21.1").unwrap();
        // a applies [A, Y, Z] and crashes before committing the baseline.
        let fail = || Err(io::Error::other("power cut"));
        {
            let s = store::lock(&w.data_root);
            let seams = Seams {
                after_instance_write: &fail,
                ..Seams::none()
            };
            assert!(sync_kind(
                &w.a,
                &s,
                &facts("1.21.1"),
                Kind::Servers,
                Join::UseShared,
                &seams
            )
            .is_err());
        }
        // Meanwhile b removes Z.
        put_servers(&w.b, &["A", "Y"]);
        sync(&w, &w.b, Kind::Servers, "1.21.1").unwrap();
        sync(&w, &w.a, Kind::Servers, "1.21.1").unwrap();
        assert_eq!(shared_servers(&w), ["A", "Y"], "Z is not resurrected");
        assert_eq!(servers_of(&w.a), ["A", "Y"]);
    }

    #[test]
    fn history_recaptured_after_a_crash_is_not_doubled() {
        let w = world();
        let p = game_file(&w.a, Kind::History);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "/a\n").unwrap();
        sync(&w, &w.a, Kind::History, "1.21.1").unwrap();
        std::fs::write(&p, "/a\n/b\n").unwrap();
        let fail = || Err(io::Error::other("power cut"));
        {
            let s = store::lock(&w.data_root);
            let seams = Seams {
                after_capture_save: &fail,
                ..Seams::none()
            };
            assert!(sync_kind(
                &w.a,
                &s,
                &facts("1.21.1"),
                Kind::History,
                Join::UseShared,
                &seams
            )
            .is_err());
        }
        sync(&w, &w.a, Kind::History, "1.21.1").unwrap();
        let shared = store::lock(&w.data_root)
            .load(Kind::History)
            .unwrap()
            .unwrap();
        assert_eq!(String::from_utf8(shared).unwrap().matches("/b").count(), 1);
    }

    #[test]
    fn an_old_game_does_not_sync_history_or_unrecorded_hotbars() {
        let w = world();
        assert_eq!(
            sync(&w, &w.a, Kind::History, "1.20.1").unwrap(),
            KindOutcome::Unsupported {
                why: Unsupported::HistoryTooOld
            }
        );
        let s = store::lock(&w.data_root);
        let old = VersionFacts {
            mc_version: "1.12.2".into(),
            data: DataVersion::NotRecorded,
            release: Some(true),
            loader: crate::instances::schema::LoaderKind::Vanilla,
        };
        assert_eq!(
            sync_kind(
                &w.a,
                &s,
                &old,
                Kind::Hotbars,
                Join::UseShared,
                &Seams::none()
            )
            .unwrap(),
            KindOutcome::Unsupported {
                why: Unsupported::HotbarsNotRecorded
            }
        );
    }

    #[test]
    fn an_instance_older_than_the_shared_hotbars_keeps_its_own() {
        let w = world();
        let newer = Hotbars {
            rows: [("0".to_string(), Value::List(vec![]))]
                .into_iter()
                .collect(),
            data_version: Some(4300),
            framing: Framing::Raw,
        };
        store::lock(&w.data_root)
            .save(Kind::Hotbars, &hotbars::to_bytes(&newer).unwrap())
            .unwrap();
        let mine = Hotbars {
            data_version: Some(3955),
            ..newer.clone()
        };
        let p = game_file(&w.a, Kind::Hotbars);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let before = hotbars::to_bytes(&mine).unwrap();
        std::fs::write(&p, &before).unwrap();
        assert_eq!(
            sync(&w, &w.a, Kind::Hotbars, "1.21.1").unwrap(),
            KindOutcome::OlderThanShared
        );
        assert_eq!(std::fs::read(&p).unwrap(), before, "untouched");
    }
}

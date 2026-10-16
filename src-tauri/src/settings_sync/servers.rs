//! servers.dat (spec §B4): the projection, what changed since the baseline,
//! the merge into the shared list, and the list an instance gets back.
//!
//! An entry is identified by its normalised address plus its occurrence
//! among entries with that address: the game allows duplicates, and keying by
//! address alone would collapse them.

use std::collections::HashMap;
use std::path::Path;

use fastnbt::Value;

use crate::error::{Error, Result};

pub(crate) type Entry = HashMap<String, Value>;
type Key = (String, usize);

/// The game's default port: `host` and `host:25565` are one server.
const DEFAULT_PORT: &str = ":25565";

/// `host[:port]`, lower-cased host, the default port dropped.
pub(crate) fn norm_addr(addr: &str) -> String {
    let a = addr.trim().to_ascii_lowercase();
    a.strip_suffix(DEFAULT_PORT)
        .map(str::to_string)
        .unwrap_or(a)
}

fn address(e: &Entry) -> String {
    match e.get("ip") {
        Some(Value::String(ip)) => norm_addr(ip),
        _ => String::new(),
    }
}

fn keys(entries: &[Entry]) -> Vec<Key> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    entries
        .iter()
        .map(|e| {
            let a = address(e);
            let n = seen.entry(a.clone()).or_insert(0);
            let k = (a, *n);
            *n += 1;
            k
        })
        .collect()
}

/// The instance's list; `None` when the file does not exist ("nothing saved
/// here", never "emptied" — a game killed mid-save leaves `servers.dat_old`
/// beside a missing `servers.dat`, and that captures nothing either).
pub(crate) fn read(path: &Path) -> Result<Option<Vec<Entry>>> {
    let Some(bytes) = super::fsio::read_if_present(path)? else {
        return Ok(None);
    };
    parse(&bytes)
        .map(Some)
        .map_err(|why| Error::io(path.display().to_string(), why))
}

fn parse(bytes: &[u8]) -> std::result::Result<Vec<Entry>, String> {
    let root: Value = fastnbt::from_bytes(bytes).map_err(|e| format!("not NBT: {e}"))?;
    from_root(root)
}

fn from_root(root: Value) -> std::result::Result<Vec<Entry>, String> {
    let Value::Compound(mut map) = root else {
        return Err("the root is not a compound".into());
    };
    match map.remove("servers") {
        None => Ok(Vec::new()),
        Some(list) => from_value(list),
    }
}

/// A projection stored in a baseline: a list of compounds.
pub(crate) fn from_value(v: Value) -> std::result::Result<Vec<Entry>, String> {
    let Value::List(items) = v else {
        return Err("the server list is not a list".into());
    };
    items
        .into_iter()
        .map(|i| match i {
            Value::Compound(c) => Ok(c),
            _ => Err("a server entry is not a compound".to_string()),
        })
        .collect()
}

pub(crate) fn to_value(entries: &[Entry]) -> Value {
    Value::List(entries.iter().cloned().map(Value::Compound).collect())
}

/// `servers.dat` bytes: `{servers: [...]}`, uncompressed, as the game writes it.
pub(crate) fn to_bytes(entries: &[Entry]) -> Result<Vec<u8>> {
    let mut root = HashMap::new();
    root.insert("servers".to_string(), to_value(entries));
    fastnbt::to_bytes(&Value::Compound(root))
        .map_err(|e| Error::io("servers.dat", format!("serialize: {e}")))
}

/// The shared list being merged into, each entry with its key.
struct Merging(Vec<(Key, Entry)>);

impl Merging {
    fn new(shared: &[Entry]) -> Self {
        Self(
            keys(shared)
                .into_iter()
                .zip(shared.iter().cloned())
                .collect(),
        )
    }

    fn position(&self, key: &Key) -> Option<usize> {
        self.0.iter().position(|(k, _)| k == key)
    }

    /// Insert `entry` after the nearest entry before it in `order` that the
    /// shared list has, else at the end. A key the list already has is left
    /// alone (another instance added the same server).
    fn add(&mut self, order: &[Key], at: usize, entry: &Entry) {
        let key = &order[at];
        if self.position(key).is_some() {
            return;
        }
        let after = order[..at].iter().rev().find_map(|k| self.position(k));
        let slot = after.map_or(self.0.len(), |p| p + 1);
        self.0.insert(slot, (key.clone(), entry.clone()));
    }

    fn into_entries(self) -> Vec<Entry> {
        self.0.into_iter().map(|(_, e)| e).collect()
    }
}

/// Merge what changed in the instance since `baseline` into `shared`:
/// removals remove, a field the instance changed or added overwrites, adds
/// land after their nearest neighbour, and a reorder here reorders the
/// shared entries the instance has. A field present in the baseline and
/// missing now is not a change: an older game drops what it does not know.
pub(crate) fn capture(baseline: &[Entry], now: &[Entry], shared: &[Entry]) -> Vec<Entry> {
    let base_keys = keys(baseline);
    let now_keys = keys(now);
    let base: HashMap<&Key, &Entry> = base_keys.iter().zip(baseline).collect();
    let here: HashMap<&Key, &Entry> = now_keys.iter().zip(now).collect();
    let mut out = Merging::new(shared);

    out.0
        .retain(|(k, _)| !base.contains_key(k) || here.contains_key(k));

    for (key, entry) in now_keys.iter().zip(now) {
        let Some(before) = base.get(key) else {
            continue;
        };
        let Some(p) = out.position(key) else {
            continue;
        };
        for (field, value) in entry {
            if before.get(field) != Some(value) {
                out.0[p].1.insert(field.clone(), value.clone());
            }
        }
    }

    for (at, entry) in now.iter().enumerate() {
        if !base.contains_key(&now_keys[at]) {
            out.add(&now_keys, at, entry);
        }
    }

    // A reorder: the slots the instance's entries hold in the shared list
    // take those entries in the instance's order.
    let mut slots: Vec<usize> = now_keys.iter().filter_map(|k| out.position(k)).collect();
    slots.sort_unstable();
    let ordered: Vec<(Key, Entry)> = now_keys
        .iter()
        .filter_map(|k| out.position(k).map(|p| out.0[p].clone()))
        .collect();
    for (slot, item) in slots.into_iter().zip(ordered) {
        out.0[slot] = item;
    }

    out.into_entries()
}

/// Add every instance entry the shared list does not have (a join: nothing
/// is lost, nothing is removed, nothing reordered).
pub(crate) fn union(shared: &[Entry], instance: &[Entry]) -> Vec<Entry> {
    let inst_keys = keys(instance);
    let mut out = Merging::new(shared);
    for (at, entry) in instance.iter().enumerate() {
        out.add(&inst_keys, at, entry);
    }
    out.into_entries()
}

/// The list an instance gets: the shared one, without `hidden` entries for a
/// game older than 1.19 (it would show them).
pub(crate) fn render(shared: &[Entry], shows_hidden: bool) -> Vec<Entry> {
    shared
        .iter()
        .filter(|e| shows_hidden || !matches!(e.get("hidden"), Some(Value::Byte(1))))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, ip: &str) -> Entry {
        let mut m = HashMap::new();
        m.insert("name".into(), Value::String(name.into()));
        m.insert("ip".into(), Value::String(ip.into()));
        m
    }

    fn with(mut entry: Entry, field: &str, v: Value) -> Entry {
        entry.insert(field.into(), v);
        entry
    }

    fn names(list: &[Entry]) -> Vec<String> {
        list.iter()
            .map(|e| match e.get("name") {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            })
            .collect()
    }

    #[test]
    fn localhost_and_its_default_port_are_one_server() {
        assert_eq!(norm_addr("LocalHost:25565"), norm_addr("localhost"));
        assert_ne!(norm_addr("localhost:25566"), norm_addr("localhost"));
    }

    #[test]
    fn an_added_server_lands_after_its_neighbour() {
        let base = vec![e("A", "a"), e("B", "b")];
        let now = vec![e("A", "a"), e("N", "n"), e("B", "b")];
        let shared = vec![e("X", "x"), e("A", "a"), e("B", "b")];
        assert_eq!(names(&capture(&base, &now, &shared)), ["X", "A", "N", "B"]);
    }

    #[test]
    fn a_removed_server_leaves_the_shared_list() {
        let base = vec![e("A", "a"), e("B", "b")];
        let now = vec![e("A", "a")];
        let shared = vec![e("A", "a"), e("B", "b"), e("C", "c")];
        assert_eq!(names(&capture(&base, &now, &shared)), ["A", "C"]);
    }

    #[test]
    fn a_renamed_server_is_renamed_in_the_shared_list() {
        let base = vec![e("A", "a")];
        let now = vec![e("Alpha", "a")];
        let shared = vec![e("A", "a"), e("B", "b")];
        assert_eq!(names(&capture(&base, &now, &shared)), ["Alpha", "B"]);
    }

    #[test]
    fn a_reorder_here_reorders_the_shared_entries_it_has() {
        let base = vec![e("A", "a"), e("B", "b")];
        let now = vec![e("B", "b"), e("A", "a")];
        let shared = vec![e("A", "a"), e("X", "x"), e("B", "b")];
        assert_eq!(names(&capture(&base, &now, &shared)), ["B", "X", "A"]);
    }

    #[test]
    fn a_field_an_older_game_dropped_is_not_a_deletion() {
        let flagged = with(e("A", "a"), "acceptTextures", Value::Byte(1));
        let base = vec![flagged.clone()];
        let now = vec![e("A", "a")];
        let shared = vec![flagged.clone()];
        assert_eq!(capture(&base, &now, &shared), vec![flagged]);
    }

    #[test]
    fn a_field_the_game_added_travels() {
        let base = vec![e("A", "a")];
        let now = vec![with(e("A", "a"), "acceptTextures", Value::Byte(1))];
        let shared = vec![e("A", "a")];
        assert_eq!(capture(&base, &now, &shared), now);
    }

    #[test]
    fn duplicate_addresses_are_kept_apart() {
        let base = vec![e("L1", "localhost"), e("L2", "localhost:25565")];
        let now = vec![
            e("L1", "localhost"),
            e("L2", "localhost:25565"),
            e("L3", "localhost"),
        ];
        let shared = base.clone();
        assert_eq!(names(&capture(&base, &now, &shared)), ["L1", "L2", "L3"]);
    }

    #[test]
    fn a_join_adds_what_shared_lacks_and_removes_nothing() {
        let shared = vec![e("A", "a"), e("B", "b")];
        let inst = vec![e("B", "b"), e("M", "m")];
        assert_eq!(names(&union(&shared, &inst)), ["A", "B", "M"]);
    }

    #[test]
    fn hidden_servers_stay_shared_but_not_shown_to_an_old_game() {
        let hidden = with(e("H", "h"), "hidden", Value::Byte(1));
        let shared = vec![e("A", "a"), hidden];
        assert_eq!(names(&render(&shared, true)), ["A", "H"]);
        assert_eq!(names(&render(&shared, false)), ["A"]);
    }

    #[test]
    fn the_file_round_trips_and_a_missing_one_is_nothing_saved() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("servers.dat");
        assert_eq!(read(&p).unwrap(), None);
        std::fs::write(td.path().join("servers.dat_old"), b"x").unwrap();
        assert_eq!(read(&p).unwrap(), None, "a leftover _old captures nothing");
        let list = vec![e("A", "a"), with(e("H", "h"), "hidden", Value::Byte(1))];
        std::fs::write(&p, to_bytes(&list).unwrap()).unwrap();
        assert_eq!(read(&p).unwrap(), Some(list));
    }
}

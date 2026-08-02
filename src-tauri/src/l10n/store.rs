//! The global override store: a user's own translations, keyed by resource
//! NAMESPACE rather than by jar or by instance.
//!
//! Namespace, not jar, because a namespace is what a resource pack overrides,
//! one jar may ship several (Jar-in-Jar), and the same mod version carries
//! identical strings in every instance — so a translation written once
//! applies everywhere it is installed. Path: `<store_dir>/<lang>/<namespace>.json`.
//!
//! Every entry records the English string it was written against
//! (`Entry::source_en`) — the staleness oracle. Without it a mod update
//! silently invalidates a translation and nobody can tell which one: an
//! unchanged English string means the translation still holds, a changed one
//! means it needs review, and a vanished key means it is an orphan (see
//! `state_of`).
//!
//! `Entry::origin` is written from day one — `Manual` now, `Machine` for the
//! deferred AI-assist stage — so that stage adds a value to an existing enum
//! rather than a schema migration.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::l10n::scan::LangMap;

const FILE_VERSION: u32 = 1;

/// Bumped once per atomic write so no two writes in one process can share a
/// temp path. See [`temp_suffix`].
static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// The extension suffix for an atomic write's temp file: process id **and** a
/// per-call sequence number.
///
/// The process id alone makes the name unique across launcher processes but
/// not across concurrent callers inside one, and two of those writing the same
/// destination share the same temp path — so one can rename a half-written
/// file into place. The AI pre-fill made that reachable: the override store
/// and its answer cache are global (`<store_dir>/<lang>/…`), not
/// instance-scoped, and two instances pre-filling at once is an expected
/// state. Same shape as `mods::installed`'s and `l10n::options_txt`'s
/// `WRITE_SEQ`.
pub(crate) fn temp_suffix() -> String {
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("tmp.{}.{seq}", std::process::id())
}

/// Where a translation came from. Written from day one so the machine-
/// translation stage adds a value rather than a schema migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Manual,
    Machine,
}

impl Default for Origin {
    fn default() -> Self {
        Self::Manual
    }
}

/// One user-authored translation.
///
/// Deliberately NOT `Eq`: `updated_at` is an `f64`, and `f64` has no total
/// order (NaN), so `#[derive(Eq)]` on a struct containing one cannot compile.
/// `JournalEntry` in `journal/mod.rs` — the other struct in this crate with a
/// millisecond-timestamp `f64` field — omits `Eq` for the identical reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub value: String,
    /// The English string this translation was written against — the
    /// staleness oracle. Without it a mod update silently invalidates
    /// translations and nobody can tell which.
    pub source_en: String,
    #[serde(default)]
    pub origin: Origin,
    /// Unix milliseconds. `f64` because specta forbids 64-bit integer
    /// exports; unix-ms overflows `f64`'s 53-bit exact-integer range only in
    /// the year 287396, so precision is not a concern in practice. Mirrors
    /// `JournalEntry::at_unix_ms` in `journal/mod.rs`.
    pub updated_at: f64,
}

/// What the editor shows for one key, combining the user's override (if any)
/// with what the mod itself ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    /// No override; the mod's own target-language file already has this key.
    FromMod,
    /// Override present and still matches the mod's current English string.
    Ok,
    /// Override present but the mod's English string has changed since it
    /// was written — needs review.
    Stale,
    /// Override present but the key no longer exists in the mod's English
    /// file at all.
    Orphan,
    /// No override, and the mod does not translate this key either.
    Missing,
}

/// One namespace's worth of user overrides for one target language. This is
/// the on-disk shape (`<store_dir>/<lang>/<namespace>.json`), not an IPC
/// return type — `Entry`/`Origin`/`KeyState` cross IPC as fields of [`KeyRow`]
/// below, but this container never does (mirrors `AccountFile` in `accounts/store.rs`,
/// which likewise skips `specta::Type` while its element type `Account`
/// carries it). It also has no `#[serde(rename_all)]`, matching that same
/// precedent: an on-disk-only struct here keeps plain Rust field names on
/// disk rather than a camelCase IPC convention it never needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamespaceStore {
    pub version: u32,
    pub namespace: String,
    pub lang: String,
    #[serde(default)]
    pub entries: BTreeMap<String, Entry>,
}

impl NamespaceStore {
    pub fn new(namespace: impl Into<String>, lang: impl Into<String>) -> Self {
        Self {
            version: FILE_VERSION,
            namespace: namespace.into(),
            lang: lang.into(),
            entries: BTreeMap::new(),
        }
    }

    /// Record or replace the override for `key`. Always writes `Origin::Manual`
    /// — this is the hand-edit path; the machine-translation stage has its own
    /// write path, [`set_with_origin`](Self::set_with_origin).
    pub fn set(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
        source_en: impl Into<String>,
        updated_at_ms: f64,
    ) {
        self.set_with_origin(key, value, source_en, updated_at_ms, Origin::Manual);
    }

    /// Write an entry with an explicit origin. The machine-translation stage
    /// writes `Origin::Machine`; the hand-edit path goes through [`set`](Self::set)
    /// and stays `Manual`, so a user editing a machine string reclaims it.
    pub fn set_with_origin(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
        source_en: impl Into<String>,
        updated_at_ms: f64,
        origin: Origin,
    ) {
        self.entries.insert(
            key.into(),
            Entry {
                value: value.into(),
                source_en: source_en.into(),
                origin,
                updated_at: updated_at_ms,
            },
        );
    }

    /// Delete the override for `key`, if any. Purely in-memory — callers that
    /// want the change to survive must still call `save`.
    pub fn remove(&mut self, key: &str) -> Option<Entry> {
        self.entries.remove(key)
    }

    /// Classify one key for the editor, given the mod's current English map
    /// and (optionally) the mod's own translation into the target language.
    ///
    /// An override always wins over the mod's own translation — checked
    /// first, unconditionally — because letting a user's explicit choice be
    /// shadowed by whatever the mod ships would defeat the point of the
    /// feature.
    pub fn state_of(&self, key: &str, en: &LangMap, mod_tr: Option<&LangMap>) -> KeyState {
        if let Some(entry) = self.entries.get(key) {
            return match en.get(key) {
                None => KeyState::Orphan,
                Some(current_en) if current_en == &entry.source_en => KeyState::Ok,
                Some(_) => KeyState::Stale,
            };
        }
        if mod_tr.is_some_and(|m| m.contains_key(key)) {
            return KeyState::FromMod;
        }
        KeyState::Missing
    }
}

/// One key's editor row: what the mod ships (English + its own target-language
/// translation, if any), what the user overrode (if anything), and the
/// combined [`KeyState`]. Crosses IPC — this is `commands::l10n_namespace_keys`'s
/// element type.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KeyRow {
    pub key: String,
    pub source_en: String,
    pub mod_value: Option<String>,
    pub override_value: Option<String>,
    pub state: KeyState,
    /// Where the override came from, or `None` when the key has no override.
    /// Drives the editor's manual/machine filter and the bulk-revert action.
    pub origin: Option<Origin>,
}

/// Every key the editor should show for one namespace.
///
/// The key UNIVERSE is `en` — every key in the mod's own English file — PLUS
/// any override whose key `en` no longer has: an orphan the user must still
/// be able to find and clear (see the module doc on `Entry::source_en`, the
/// staleness oracle this exists to serve). Without appending orphans
/// explicitly here, a translation for a key the mod dropped would become
/// permanently invisible to the user who wrote it.
///
/// For an orphan row, `source_en` is the entry's OWN recorded
/// `Entry::source_en` (what it was translated against) rather than an empty
/// string — showing "you translated *(was: 'Old Text')*" is far more useful
/// than a blank field, and it is exactly the value `state_of` already uses to
/// detect the key is an orphan in the first place.
///
/// `mod_tr` mirrors [`state_of`]'s parameter: the mod's own target-language
/// file for this namespace, if any. Rows are returned in key-sorted order —
/// deterministic, not meaningful UI ordering; the caller sorts however it
/// likes.
pub fn namespace_key_rows(
    store: &NamespaceStore,
    en: &LangMap,
    mod_tr: Option<&LangMap>,
) -> Vec<KeyRow> {
    let keys: BTreeSet<&str> = en
        .keys()
        .map(String::as_str)
        .chain(store.entries.keys().map(String::as_str))
        .collect();

    keys.into_iter()
        .map(|key| {
            let entry = store.entries.get(key);
            let source_en = en
                .get(key)
                .cloned()
                .unwrap_or_else(|| entry.map(|e| e.source_en.clone()).unwrap_or_default());
            KeyRow {
                key: key.to_string(),
                source_en,
                mod_value: mod_tr.and_then(|m| m.get(key)).cloned(),
                override_value: entry.map(|e| e.value.clone()),
                state: store.state_of(key, en, mod_tr),
                origin: entry.map(|e| e.origin),
            }
        })
        .collect()
}

/// Percent-encode `s` so it is always safe to compose into a filesystem path
/// component AND can never collide with a different input's encoding.
///
/// Every ASCII lowercase letter, digit, `_`, `.` and `-` passes through
/// unchanged — together they cover the legal grammar for a Minecraft
/// resource namespace (`[a-z0-9_.-]`), so a well-formed namespace produces a
/// human-readable file name. Every other byte — including uppercase (the
/// grammar disallows it, but nothing stops a malformed jar from shipping it;
/// `l10n::scan` does not lowercase `namespace`, only `code`), `/`, `\`, and
/// the escape character `%` itself — is replaced with `%XX` (uppercase hex of
/// the raw byte).
///
/// Escaping `%` too is what makes this injective, and injectivity is what
/// makes it collision-free: if `%` passed through unescaped, both `"a%2Fb"`
/// and `"a/b"` would sanitise to `a%2Fb`. With `%` itself escaped, the only
/// input that can produce a literal `%2F` in the output is one that already
/// contained an actual `/` byte, so two distinct namespaces can never
/// collapse onto the same file. This is precisely what fixes the original
/// draft's collision: it mapped every non-alphanumeric character (including
/// both `.` and `_`) to `_`, so `some.mod` and `some_mod` — two different,
/// real namespaces — landed on the same `some_mod.json`. Here `.` is in the
/// pass-through set, so `some.mod` stays `some.mod` and the two remain
/// distinct (`sanitise_never_collides_dot_and_underscore_namespaces` below).
///
/// A path separator is never in the pass-through set, so the output can never
/// contain an unescaped `/` or `\` — a namespace like `"../../evil"` sanitises
/// to `"..%2F..%2Fevil"`, a single ordinary path component with no separator
/// in it, not two directory hops out of the store. `l10n::scan::is_traversal_unsafe`
/// (which rejects bare `.`/`..`, backslashes, and control characters at the
/// jar-scan boundary) is deliberately NOT reused here: it is a reject-or-keep
/// predicate for a boundary that drops bad input, whereas `store_path` must
/// be total — every `&str` has to produce *some* valid path — and a
/// blocklist of specific bad strings does nothing to solve collision-safety,
/// which is the harder half of this function's job.
fn sanitise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `<store_dir>/<lang>/<namespace>.json`, with both path components
/// sanitised. `lang` is sanitised too, defensively: it is ultimately drawn
/// from the same jar-derived code list (`l10n::scan::LangEntry::code`,
/// `InstanceCoverage::available_codes`) as `namespace`, so treating it as
/// more trustworthy than `namespace` would just relocate the same hazard.
pub fn store_path(store_dir: &Path, lang: &str, namespace: &str) -> PathBuf {
    store_dir
        .join(sanitise(lang))
        .join(format!("{}.json", sanitise(namespace)))
}

/// Load one namespace's overrides. A missing OR malformed file yields an
/// empty store rather than an error — the editor modal must still open if
/// one file on disk is damaged, the same tolerance `ScanCache::load` and
/// `read_account_file` already extend to their own on-disk state.
pub fn load(store_dir: &Path, lang: &str, namespace: &str) -> NamespaceStore {
    let path = store_path(store_dir, lang, namespace);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return NamespaceStore::new(namespace, lang);
    };
    serde_json::from_str(&raw).unwrap_or_else(|_| NamespaceStore::new(namespace, lang))
}

/// Persist one namespace's overrides atomically (temp file + rename), creating
/// the language directory if needed.
///
/// The temp name is unique per **call** (see [`temp_suffix`]), not merely per
/// process. That stopped being optional with the AI pre-fill: this file is
/// global rather than instance-scoped, so two runs in one process can save the
/// same `(lang, namespace)` at the same time, and a shared temp path lets one
/// rename the other's half-written file into place.
///
/// This rewrites the WHOLE file from `store`. A caller holding a long-lived
/// snapshot must re-[`load`] before saving, or it will delete every entry
/// written by anyone else since it read.
pub fn save(store_dir: &Path, store: &NamespaceStore) -> std::io::Result<()> {
    let path = store_path(store_dir, &store.lang, &store.namespace);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(store)?;
    let tmp = path.with_extension(temp_suffix());
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Every namespace under `<store_dir>/<lang>/` that currently has at least
/// one override, sorted. A directory that does not exist yet (no overrides
/// ever saved for this language) yields an empty list rather than an error.
///
/// A file that parses but whose `entries` map is empty — the last override
/// for that namespace was `remove`d and the empty store was then saved — is
/// excluded: an on-disk file is not by itself evidence of a live override,
/// and reporting it as one would be a false positive in whatever UI badge
/// this feeds. A file that fails to parse at all is skipped the same way
/// `load` treats a malformed file: as absent, not as an error.
pub fn namespaces_with_overrides(store_dir: &Path, lang: &str) -> Vec<String> {
    let dir = store_dir.join(sanitise(lang));
    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = read_dir
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|raw| serde_json::from_str::<NamespaceStore>(&raw).ok())
        .filter(|store| !store.entries.is_empty())
        .map(|store| store.namespace)
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn map(pairs: &[(&str, &str)]) -> LangMap {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    // -----------------------------------------------------------------
    // sanitise / store_path
    // -----------------------------------------------------------------

    #[test]
    fn sanitise_never_collides_dot_and_underscore_namespaces() {
        // The exact collision the original draft sanitiser produced: mapping
        // every non-alphanumeric character to '_' sends both `some.mod` and
        // `some_mod` to `some_mod.json`, silently merging two mods'
        // translations. `.` must stay a pass-through character so these two
        // real, distinct namespaces land on different files.
        let dir = Path::new("store");
        let a = store_path(dir, "ru_ru", "some.mod");
        let b = store_path(dir, "ru_ru", "some_mod");
        assert_ne!(a, b);
        assert_eq!(a.file_name().unwrap().to_str().unwrap(), "some.mod.json");
        assert_eq!(b.file_name().unwrap().to_str().unwrap(), "some_mod.json");
    }

    #[test]
    fn sanitise_escapes_its_own_escape_character() {
        // If '%' passed through unescaped, "a%2Fb" and "a/b" would both
        // sanitise to "a%2Fb". Escaping '%' too is what keeps the mapping
        // injective.
        let dir = Path::new("store");
        let a = store_path(dir, "ru_ru", "a%2Fb");
        let b = store_path(dir, "ru_ru", "a/b");
        assert_ne!(a, b);
    }

    #[test]
    fn sanitise_keeps_traversal_segments_inside_the_store_dir() {
        let dir = Path::new("store");
        let path = store_path(dir, "ru_ru", "../../evil");
        // Still directly under store/ru_ru — no unescaped separator ever
        // reached the filesystem call, so there is nothing for the OS to
        // walk up through.
        assert_eq!(path.parent().unwrap(), dir.join("ru_ru"));
        assert!(path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("..%2F..%2Fevil"));
    }

    #[test]
    fn store_path_is_lang_then_namespace_json() {
        let dir = Path::new("store");
        let path = store_path(dir, "ru_ru", "create");
        assert_eq!(path, dir.join("ru_ru").join("create.json"));
    }

    // -----------------------------------------------------------------
    // load / save
    // -----------------------------------------------------------------

    #[test]
    fn round_trips_through_disk() {
        let dir = tempdir().unwrap();
        let mut store = NamespaceStore::new("create", "ru_ru");
        store.set("item.create.wrench", "Гаечный ключ", "Wrench", 1_000.0);

        save(dir.path(), &store).unwrap();
        let reloaded = load(dir.path(), "ru_ru", "create");

        assert_eq!(reloaded, store);
    }

    #[test]
    fn missing_file_yields_an_empty_store() {
        let dir = tempdir().unwrap();
        let store = load(dir.path(), "ru_ru", "create");
        assert!(store.entries.is_empty());
        assert_eq!(store.namespace, "create");
        assert_eq!(store.lang, "ru_ru");
    }

    #[test]
    fn malformed_file_yields_an_empty_store_not_an_error() {
        // The modal must still open even if one file on disk is damaged.
        let dir = tempdir().unwrap();
        let path = store_path(dir.path(), "ru_ru", "create");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();

        let store = load(dir.path(), "ru_ru", "create");
        assert!(store.entries.is_empty());
    }

    #[test]
    fn languages_and_namespaces_are_separate_files() {
        let dir = tempdir().unwrap();

        let mut create_ru = NamespaceStore::new("create", "ru_ru");
        create_ru.set("a", "А", "A", 1.0);
        save(dir.path(), &create_ru).unwrap();

        let mut create_de = NamespaceStore::new("create", "de_de");
        create_de.set("a", "Ein", "A", 1.0);
        save(dir.path(), &create_de).unwrap();

        let mut thermal_ru = NamespaceStore::new("thermal", "ru_ru");
        thermal_ru.set("b", "Б", "B", 1.0);
        save(dir.path(), &thermal_ru).unwrap();

        // Same namespace, different language: distinct files, no bleed.
        assert_eq!(load(dir.path(), "ru_ru", "create"), create_ru);
        assert_eq!(load(dir.path(), "de_de", "create"), create_de);
        // Same language, different namespace: distinct files, no bleed.
        assert_eq!(load(dir.path(), "ru_ru", "thermal"), thermal_ru);
        assert_ne!(
            load(dir.path(), "ru_ru", "create").entries,
            load(dir.path(), "ru_ru", "thermal").entries
        );
    }

    #[test]
    fn save_creates_the_language_directory() {
        let dir = tempdir().unwrap();
        let store = NamespaceStore::new("create", "ru_ru");
        save(dir.path(), &store).unwrap();
        assert!(dir.path().join("ru_ru").is_dir());
    }

    // -----------------------------------------------------------------
    // set / remove
    // -----------------------------------------------------------------

    #[test]
    fn set_records_manual_origin_and_the_source_english_it_was_written_against() {
        let mut store = NamespaceStore::new("create", "ru_ru");
        store.set("item.create.wrench", "Гаечный ключ", "Wrench", 42.0);

        let entry = store.entries.get("item.create.wrench").unwrap();
        assert_eq!(entry.value, "Гаечный ключ");
        assert_eq!(entry.source_en, "Wrench");
        assert_eq!(entry.origin, Origin::Manual);
        assert_eq!(entry.updated_at, 42.0);
    }

    #[test]
    fn set_with_origin_records_machine_and_set_stays_manual() {
        let mut s = NamespaceStore::new("create", "ru_ru");
        s.set_with_origin("block.a", "Блок", "Block", 1.0, Origin::Machine);
        s.set("block.b", "Блок Б", "Block B", 2.0);
        assert_eq!(s.entries["block.a"].origin, Origin::Machine);
        assert_eq!(s.entries["block.b"].origin, Origin::Manual);
    }

    #[test]
    fn set_replaces_a_prior_entry_for_the_same_key() {
        let mut store = NamespaceStore::new("create", "ru_ru");
        store.set("a", "first", "A", 1.0);
        store.set("a", "second", "A", 2.0);
        assert_eq!(store.entries.len(), 1);
        assert_eq!(store.entries["a"].value, "second");
    }

    #[test]
    fn remove_deletes_an_entry() {
        let mut store = NamespaceStore::new("create", "ru_ru");
        store.set("a", "А", "A", 1.0);
        assert!(store.entries.contains_key("a"));

        let removed = store.remove("a");
        assert!(removed.is_some());
        assert!(!store.entries.contains_key("a"));
    }

    #[test]
    fn remove_of_an_absent_key_is_a_harmless_none() {
        let mut store = NamespaceStore::new("create", "ru_ru");
        assert_eq!(store.remove("nope"), None);
    }

    // -----------------------------------------------------------------
    // state_of
    // -----------------------------------------------------------------

    #[test]
    fn state_is_from_mod_when_no_override_and_the_mod_translates_it() {
        let store = NamespaceStore::new("create", "ru_ru");
        let en = map(&[("a", "A")]);
        let mod_tr = map(&[("a", "А")]);
        assert_eq!(store.state_of("a", &en, Some(&mod_tr)), KeyState::FromMod);
    }

    #[test]
    fn state_is_missing_when_no_override_and_the_mod_does_not_translate_it_either() {
        let store = NamespaceStore::new("create", "ru_ru");
        let en = map(&[("a", "A")]);
        assert_eq!(store.state_of("a", &en, None), KeyState::Missing);

        let mod_tr = map(&[("other", "X")]);
        assert_eq!(store.state_of("a", &en, Some(&mod_tr)), KeyState::Missing);
    }

    #[test]
    fn state_is_ok_when_the_override_still_matches_current_english() {
        let mut store = NamespaceStore::new("create", "ru_ru");
        store.set("a", "А", "A", 1.0);
        let en = map(&[("a", "A")]);
        assert_eq!(store.state_of("a", &en, None), KeyState::Ok);
    }

    #[test]
    fn state_is_stale_when_current_english_has_changed() {
        let mut store = NamespaceStore::new("create", "ru_ru");
        store.set("a", "А", "A", 1.0);
        let en = map(&[("a", "A (renamed)")]);
        assert_eq!(store.state_of("a", &en, None), KeyState::Stale);
    }

    #[test]
    fn state_is_orphan_when_the_key_vanished_from_english() {
        let mut store = NamespaceStore::new("create", "ru_ru");
        store.set("a", "А", "A", 1.0);
        let en = map(&[("other", "X")]); // "a" is gone
        assert_eq!(store.state_of("a", &en, None), KeyState::Orphan);
    }

    #[test]
    fn an_override_wins_over_the_mods_own_translation() {
        // 'a' is both overridden AND translated by the mod: the override
        // must decide the state, not FromMod — that is the entire point of
        // the feature.
        let mut store = NamespaceStore::new("create", "ru_ru");
        store.set("a", "Мой перевод", "A", 1.0);
        let en = map(&[("a", "A")]);
        let mod_tr = map(&[("a", "Перевод мода")]);
        assert_eq!(store.state_of("a", &en, Some(&mod_tr)), KeyState::Ok);
    }

    #[test]
    fn an_override_wins_over_the_mods_translation_even_when_stale() {
        // Same as above but the override is stale: still not FromMod.
        let mut store = NamespaceStore::new("create", "ru_ru");
        store.set("a", "Мой перевод", "old A", 1.0);
        let en = map(&[("a", "new A")]);
        let mod_tr = map(&[("a", "Перевод мода")]);
        assert_eq!(store.state_of("a", &en, Some(&mod_tr)), KeyState::Stale);
    }

    // -----------------------------------------------------------------
    // namespaces_with_overrides
    // -----------------------------------------------------------------

    #[test]
    fn lists_namespaces_with_overrides_sorted() {
        let dir = tempdir().unwrap();

        let mut thermal = NamespaceStore::new("thermal", "ru_ru");
        thermal.set("a", "А", "A", 1.0);
        save(dir.path(), &thermal).unwrap();

        let mut create = NamespaceStore::new("create", "ru_ru");
        create.set("b", "Б", "B", 1.0);
        save(dir.path(), &create).unwrap();

        assert_eq!(
            namespaces_with_overrides(dir.path(), "ru_ru"),
            vec!["create".to_string(), "thermal".to_string()]
        );
    }

    #[test]
    fn namespaces_with_overrides_ignores_a_different_language() {
        let dir = tempdir().unwrap();
        let mut create_ru = NamespaceStore::new("create", "ru_ru");
        create_ru.set("a", "А", "A", 1.0);
        save(dir.path(), &create_ru).unwrap();

        assert!(namespaces_with_overrides(dir.path(), "de_de").is_empty());
    }

    #[test]
    fn namespaces_with_overrides_excludes_a_store_emptied_by_remove() {
        let dir = tempdir().unwrap();
        let mut store = NamespaceStore::new("create", "ru_ru");
        store.set("a", "А", "A", 1.0);
        save(dir.path(), &store).unwrap();

        store.remove("a");
        save(dir.path(), &store).unwrap();

        assert!(namespaces_with_overrides(dir.path(), "ru_ru").is_empty());
    }

    #[test]
    fn namespaces_with_overrides_on_a_missing_language_dir_is_empty() {
        let dir = tempdir().unwrap();
        assert!(namespaces_with_overrides(dir.path(), "ru_ru").is_empty());
    }

    #[test]
    fn namespaces_with_overrides_skips_a_malformed_file_without_failing() {
        let dir = tempdir().unwrap();
        let mut good = NamespaceStore::new("create", "ru_ru");
        good.set("a", "А", "A", 1.0);
        save(dir.path(), &good).unwrap();

        // `save` above already created the `ru_ru` directory.
        let broken_path = store_path(dir.path(), "ru_ru", "broken");
        std::fs::write(&broken_path, b"{ not json").unwrap();

        assert_eq!(
            namespaces_with_overrides(dir.path(), "ru_ru"),
            vec!["create".to_string()]
        );
    }

    // -----------------------------------------------------------------
    // namespace_key_rows
    // -----------------------------------------------------------------

    fn row_for<'a>(rows: &'a [KeyRow], key: &str) -> &'a KeyRow {
        rows.iter().find(|r| r.key == key).unwrap_or_else(|| {
            panic!("no row for key {key:?} in {rows:?}");
        })
    }

    #[test]
    fn every_english_key_gets_a_row_even_with_no_override_or_mod_translation() {
        let store = NamespaceStore::new("create", "ru_ru");
        let en = map(&[("a", "A"), ("b", "B")]);
        let rows = namespace_key_rows(&store, &en, None);
        assert_eq!(rows.len(), 2);
        let a = row_for(&rows, "a");
        assert_eq!(a.source_en, "A");
        assert_eq!(a.mod_value, None);
        assert_eq!(a.override_value, None);
        assert_eq!(a.state, KeyState::Missing);
    }

    #[test]
    fn a_key_the_mod_translates_reports_from_mod_and_carries_the_mod_value() {
        let store = NamespaceStore::new("create", "ru_ru");
        let en = map(&[("a", "A")]);
        let mod_tr = map(&[("a", "А")]);
        let rows = namespace_key_rows(&store, &en, Some(&mod_tr));
        let a = row_for(&rows, "a");
        assert_eq!(a.mod_value.as_deref(), Some("А"));
        assert_eq!(a.override_value, None);
        assert_eq!(a.state, KeyState::FromMod);
    }

    #[test]
    fn an_overridden_key_carries_the_override_value_and_wins_over_the_mod() {
        let mut store = NamespaceStore::new("create", "ru_ru");
        store.set("a", "Мой перевод", "A", 1.0);
        let en = map(&[("a", "A")]);
        let mod_tr = map(&[("a", "Перевод мода")]);
        let rows = namespace_key_rows(&store, &en, Some(&mod_tr));
        let a = row_for(&rows, "a");
        assert_eq!(a.override_value.as_deref(), Some("Мой перевод"));
        assert_eq!(a.mod_value.as_deref(), Some("Перевод мода"));
        assert_eq!(a.state, KeyState::Ok);
    }

    #[test]
    fn an_orphan_override_is_appended_with_its_own_recorded_source_en() {
        // The key the user translated is gone from `en` entirely. It must
        // still appear (orphans are the whole point of appending overrides
        // that `en` doesn't have), and its `source_en` must be the STALE
        // value it was written against, not an empty string — otherwise the
        // editor would show a blank source for a row the user needs to
        // review and possibly clear.
        let mut store = NamespaceStore::new("create", "ru_ru");
        store.set("gone", "Устарело", "Old English", 1.0);
        let en = map(&[("a", "A")]);
        let rows = namespace_key_rows(&store, &en, None);
        assert_eq!(rows.len(), 2); // "a" (from en) + "gone" (orphan)
        let orphan = row_for(&rows, "gone");
        assert_eq!(orphan.source_en, "Old English");
        assert_eq!(orphan.override_value.as_deref(), Some("Устарело"));
        assert_eq!(orphan.state, KeyState::Orphan);
    }

    #[test]
    fn rows_are_returned_in_key_sorted_order() {
        let store = NamespaceStore::new("create", "ru_ru");
        let en = map(&[("z", "Z"), ("a", "A"), ("m", "M")]);
        let rows = namespace_key_rows(&store, &en, None);
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, vec!["a", "m", "z"]);
    }

    #[test]
    fn key_rows_expose_origin_only_for_overridden_keys() {
        let mut s = NamespaceStore::new("create", "ru_ru");
        s.set_with_origin("block.a", "Блок", "Block", 1.0, Origin::Machine);
        let en: LangMap = [
            ("block.a".to_string(), "Block".to_string()),
            ("block.b".to_string(), "Block B".to_string()),
        ]
        .into_iter()
        .collect();
        let rows = namespace_key_rows(&s, &en, None);
        let a = rows.iter().find(|r| r.key == "block.a").expect("row a");
        let b = rows.iter().find(|r| r.key == "block.b").expect("row b");
        assert_eq!(a.origin, Some(Origin::Machine));
        assert_eq!(b.origin, None);
    }

    #[test]
    fn an_empty_namespace_with_no_overrides_yields_no_rows() {
        let store = NamespaceStore::new("create", "ru_ru");
        let en = map(&[]);
        assert!(namespace_key_rows(&store, &en, None).is_empty());
    }

    #[test]
    fn two_saves_of_the_same_file_never_share_a_temp_path() {
        // `<store_dir>/<lang>/<ns>.json` is global, not instance-scoped, so two
        // pre-fill runs in ONE process can save it concurrently. A temp name
        // keyed only on the process id would be identical for both, and the
        // loser's rename would publish the winner's half-written file.
        let a = temp_suffix();
        let b = temp_suffix();
        assert_ne!(
            a, b,
            "a temp suffix must be unique per call, not per process"
        );
        assert!(a.starts_with(&format!("tmp.{}.", std::process::id())));
    }
}

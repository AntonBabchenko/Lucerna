//! Share bundles: a dual-use zip — a real Minecraft resource pack the game
//! loads, plus a `lucerna-l10n.json` carrying the entries with provenance so
//! another Lucerna can import them into its own global override store.
//!
//! The wire contract (`BundleEntry`: `value`, `source_en`, `origin`,
//! snake_case, no timestamp) is deliberately NOT `store::Entry`, which is
//! camelCase and carries a required `updatedAt`. The interchange file does not
//! ship the exporter's clock — import stamps its own — and once shipped as
//! `schema: 1` this shape can never quietly change, so it is pinned here by
//! tests rather than inherited from a struct that is free to evolve.
//!
//! Everything here treats the file as hostile. Whole-file faults reject the
//! file; a single bad entry is skipped and counted, never fatal — one
//! malformed string must not cost the user the other nine thousand.

use std::collections::BTreeMap;
use std::io::Read;

use serde::Deserialize;

use crate::l10n::store::Origin;

pub const METADATA_NAME: &str = "lucerna-l10n.json";
pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_METADATA_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 100_000;
/// Each namespace becomes its own file in the store, and every coverage scan,
/// pack rebuild and apply-targets refresh re-reads all of them — so an import
/// planting hundreds of thousands of namespaces would be a permanent,
/// launcher-wide slowdown bought with one click.
pub const MAX_NAMESPACES: usize = 500;
pub const MAX_KEY_CHARS: usize = 512;
pub const MAX_VALUE_BYTES: usize = 32 * 1024;
pub const MAX_NOTE_CHARS: usize = 1000;
/// The note is rendered verbatim in the import dialog.
pub const NOTE_DISPLAY_CHARS: usize = 300;

/// Why a bundle was rejected as a whole. Typed rather than a message — the UI
/// localises it, the same argument `l10n::validate::FormatError` makes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum BundleError {
    NotAZip,
    /// No `lucerna-l10n.json`. `looks_like_resource_pack` (a `pack.mcmeta` was
    /// present) lets the UI add "this looks like a regular resource pack —
    /// install it as one instead" rather than a flat refusal.
    NoMetadata {
        looks_like_resource_pack: bool,
    },
    MetadataTooLarge,
    /// `schema` missing or not a positive integer.
    SchemaInvalid,
    SchemaTooNew {
        found: u32,
    },
    LangInvalid {
        lang: String,
    },
    TooManyNamespaces,
    TooManyEntries,
}

/// One entry as it appears on the wire. `origin` stays a STRING here and is
/// parsed leniently below: the store's `Origin` is a closed enum, so
/// deserializing straight into it would make one unrecognised value from a
/// future Lucerna fatal to the whole file — exactly the opposite of the
/// skip-and-count posture every other per-entry rule takes.
#[derive(Debug, Clone, Deserialize)]
struct WireEntry {
    value: String,
    source_en: String,
    #[serde(default = "default_origin")]
    origin: String,
}

fn default_origin() -> String {
    "manual".to_string()
}

/// Unknown fields are IGNORED (serde's default), so the format can grow
/// additively without a schema bump. `schema` itself is read off the raw JSON
/// before this struct is built — a file from a future Lucerna must get the
/// honest "update the launcher" answer even when the rest of its shape has
/// changed beyond what this struct can parse.
#[derive(Debug, Clone, Deserialize)]
struct WireBundle {
    lang: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    namespaces: BTreeMap<String, BTreeMap<String, WireEntry>>,
}

/// A parsed bundle holding only entries that passed every rule.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedBundle {
    pub lang: String,
    pub note: String,
    pub namespaces: BTreeMap<String, BTreeMap<String, ParsedEntry>>,
    /// Entries dropped by validation, including every entry of a skipped
    /// namespace — the import summary reports this so a bundle that silently
    /// lost half its content cannot look like a clean import.
    pub invalid: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedEntry {
    pub value: String,
    pub source_en: String,
    pub origin: Origin,
}

/// Minecraft's resource-location charset, which every legitimate
/// Lucerna-produced `lang` and namespace already satisfies. Deliberately
/// stricter than `scan::is_traversal_unsafe`: that predicate guards jar-derived
/// values the game had already accepted, whereas these two reach a store
/// directory name and the generated pack's filename, where a `:` on Windows
/// silently creates an NTFS alternate data stream instead of the named file.
fn is_mc_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-'))
}

/// Control (Cc) or format (Cf) characters — bidi overrides, zero-width joiners
/// and friends. Banned outright in KEYS, where they let a hostile bundle plant
/// a row that is invisible in the editor's key table or that renders
/// identically to a real one.
fn has_control_or_format_char(s: &str) -> bool {
    s.chars().any(|c| {
        c.is_control()
            || matches!(u32::from(c),
                0x00AD | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0xFEFF)
    })
}

fn key_ok(key: &str) -> bool {
    !key.is_empty() && key.chars().count() <= MAX_KEY_CHARS && !has_control_or_format_char(key)
}

/// Values get the looser rule: `\n` and `\t` are legitimate in Minecraft lang
/// strings (multi-line tooltips) and the override store contains them today, so
/// banning all control characters would make our own export → import round-trip
/// silently lossy.
fn value_ok(v: &str) -> bool {
    v.len() <= MAX_VALUE_BYTES && !v.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
}

fn parse_origin(s: &str) -> Option<Origin> {
    match s {
        "manual" => Some(Origin::Manual),
        "machine" => Some(Origin::Machine),
        _ => None,
    }
}

/// Truncate on a char boundary — `String::truncate` panics mid-UTF-8.
fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Open the zip, bound-read the metadata, validate everything.
///
/// The ONLY entry ever read out of the archive is `lucerna-l10n.json`; the
/// `assets/` tree exists for the game and is never extracted here, so there is
/// no zip-slip surface to defend.
pub fn parse_bundle_bytes(bytes: &[u8]) -> Result<ParsedBundle, BundleError> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| BundleError::NotAZip)?;

    let looks_like_pack = archive.by_name("pack.mcmeta").is_ok();
    let mut raw = Vec::new();
    {
        let entry = archive
            .by_name(METADATA_NAME)
            .map_err(|_| BundleError::NoMetadata {
                looks_like_resource_pack: looks_like_pack,
            })?;
        // Bound the DECOMPRESSED stream: the central directory's declared sizes
        // are attacker-controlled, so trusting them is how a zip bomb gets to
        // allocate arbitrarily. Same `.take(MAX + 1)` shape the modpack
        // manifest readers in `mods::modpack` use, for the same reason.
        entry
            .take(MAX_METADATA_BYTES + 1)
            .read_to_end(&mut raw)
            .map_err(|_| BundleError::MetadataTooLarge)?;
    }
    if raw.len() as u64 > MAX_METADATA_BYTES {
        return Err(BundleError::MetadataTooLarge);
    }

    let value: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|_| BundleError::SchemaInvalid)?;
    let schema = value
        .get("schema")
        .and_then(|s| s.as_u64())
        .ok_or(BundleError::SchemaInvalid)?;
    if schema > u64::from(SCHEMA_VERSION) {
        return Err(BundleError::SchemaTooNew {
            found: schema.min(u64::from(u32::MAX)) as u32,
        });
    }
    let wire: WireBundle = serde_json::from_value(value).map_err(|_| BundleError::SchemaInvalid)?;

    let lang = wire.lang.to_ascii_lowercase();
    if !is_mc_identifier(&lang) {
        return Err(BundleError::LangInvalid { lang: wire.lang });
    }
    if wire.namespaces.len() > MAX_NAMESPACES {
        return Err(BundleError::TooManyNamespaces);
    }
    if wire.namespaces.values().map(|m| m.len()).sum::<usize>() > MAX_ENTRIES {
        return Err(BundleError::TooManyEntries);
    }

    let mut invalid: u32 = 0;
    let mut namespaces = BTreeMap::new();
    for (ns, entries) in wire.namespaces {
        if !is_mc_identifier(&ns) {
            invalid += entries.len() as u32;
            continue;
        }
        let mut kept = BTreeMap::new();
        for (key, e) in entries {
            let origin = parse_origin(&e.origin);
            let ok = key_ok(&key)
                && value_ok(&e.value)
                && value_ok(&e.source_en)
                && origin.is_some()
                // NOTE the honest limit of this check: on import BOTH sides come
                // from the file, so it proves the entry's grammar is well formed
                // and self-consistent — NOT that the translation fits the
                // recipient's real mod string. That is what the staleness state
                // covers after import, and the game itself degrades a mismatched
                // template to raw text rather than crashing. Never describe
                // imported entries as "checked against the mod".
                && crate::l10n::validate::validate(&e.source_en, &e.value).is_ok();
            match (ok, origin) {
                (true, Some(origin)) => {
                    kept.insert(
                        key,
                        ParsedEntry {
                            value: e.value,
                            source_en: e.source_en,
                            origin,
                        },
                    );
                }
                _ => invalid += 1,
            }
        }
        if !kept.is_empty() {
            namespaces.insert(ns, kept);
        }
    }

    Ok(ParsedBundle {
        lang,
        note: truncate_chars(&wire.note, MAX_NOTE_CHARS),
        namespaces,
        invalid,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    KeepMine,
    TakeFile,
}

/// What an import WOULD do, for the confirmation dialog.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BundleSummary {
    pub lang: String,
    /// Truncated and stripped of control characters — this string is rendered
    /// verbatim in a dialog, and it came out of a file a stranger wrote.
    pub note: String,
    pub new_entries: u32,
    pub conflicts: u32,
    pub invalid: u32,
    pub namespaces: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub added: u32,
    pub replaced: u32,
    pub kept: u32,
    pub invalid: u32,
    pub lang: String,
}

/// Classify the bundle against the current store WITHOUT writing anything.
///
/// An entry whose value already equals the local one is counted as neither new
/// nor a conflict, because [`merge`] will leave it entirely alone — see the
/// rationale there.
pub fn inspect(store_dir: &std::path::Path, bundle: &ParsedBundle) -> BundleSummary {
    let mut new_entries = 0u32;
    let mut conflicts = 0u32;
    for (ns, entries) in &bundle.namespaces {
        let store = crate::l10n::store::load(store_dir, &bundle.lang, ns);
        for (key, e) in entries {
            match store.entries.get(key) {
                None => new_entries += 1,
                Some(mine) if mine.value == e.value => {}
                Some(_) => conflicts += 1,
            }
        }
    }
    let note: String = truncate_chars(&bundle.note, NOTE_DISPLAY_CHARS)
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    BundleSummary {
        lang: bundle.lang.clone(),
        note,
        new_entries,
        conflicts,
        invalid: bundle.invalid,
        namespaces: bundle.namespaces.keys().cloned().collect(),
    }
}

/// Merge the bundle into the global override store.
///
/// Per namespace: load → merge → save, tightly. Never a bundle-wide snapshot
/// written at the end — `store::save` rewrites a whole file, an AI pre-fill run
/// can be writing the same files concurrently, and a stale snapshot's save
/// deletes every entry the other writer put there in between (the same hazard
/// `store::save`'s own doc comment warns about, and the reason the pre-fill
/// flush re-reads each namespace before writing).
///
/// An entry whose value already matches is left ENTIRELY untouched, including
/// its `source_en`: that field records the English the RECIPIENT translated
/// against, and adopting the sender's — possibly from a newer mod version —
/// would silently flip the key from stale to ok without changing one visible
/// character.
///
/// A per-namespace save failure is logged and skipped rather than propagated:
/// earlier namespaces have already been written, and the user needs the summary
/// of what landed, not an error that hides it.
pub fn merge(
    store_dir: &std::path::Path,
    bundle: &ParsedBundle,
    policy: ConflictPolicy,
    now_ms: f64,
) -> ImportResult {
    let mut r = ImportResult {
        added: 0,
        replaced: 0,
        kept: 0,
        invalid: bundle.invalid,
        lang: bundle.lang.clone(),
    };
    for (ns, entries) in &bundle.namespaces {
        let mut store = crate::l10n::store::load(store_dir, &bundle.lang, ns);
        for (key, e) in entries {
            match store.entries.get(key) {
                None => {
                    store.set_with_origin(
                        key.clone(),
                        e.value.clone(),
                        e.source_en.clone(),
                        now_ms,
                        e.origin,
                    );
                    r.added += 1;
                }
                Some(mine) if mine.value == e.value => {}
                Some(_) => match policy {
                    ConflictPolicy::KeepMine => r.kept += 1,
                    ConflictPolicy::TakeFile => {
                        store.set_with_origin(
                            key.clone(),
                            e.value.clone(),
                            e.source_en.clone(),
                            now_ms,
                            e.origin,
                        );
                        r.replaced += 1;
                    }
                },
            }
        }
        if let Err(e) = crate::l10n::store::save(store_dir, &store) {
            crate::diag!("[l10n] share import: could not save {ns}: {e}");
        }
    }
    r
}

/// The default filename offered in the export save dialog.
///
/// PLURAL "translations" — one letter away from `options_txt::PACK_PREFIX`
/// ("lucerna-translation-"), and that letter is load-bearing. Every Apply
/// deletes each file in an instance's `resourcepacks/` whose name starts with
/// that prefix, and the no-Lucerna workflow tells the recipient to drop this
/// bundle into exactly that directory. Harmonising the two names would turn a
/// friend's shared file into something the next Apply silently destroys.
/// Pinned by `export_filename_never_matches_the_sweep_prefix`.
pub fn default_filename(lang: &str, date_ymd: &str) -> String {
    format!("lucerna-translations-{lang}-{date_ymd}.zip")
}

/// True when a user-chosen destination filename would be swept away by a
/// later Apply. The export command refuses such a destination outright rather
/// than writing a file that deletes itself.
pub fn dest_filename_reserved(file_name: &str) -> bool {
    file_name.starts_with(crate::l10n::options_txt::PACK_PREFIX)
}

/// Assemble the dual-use bundle: exactly the archive `pack::build` produces
/// for these stores, with `lucerna-l10n.json` appended.
///
/// The pack half is delegated rather than reimplemented, so the sparse-overlay
/// invariant (only overridden keys are ever written, or the pack would freeze
/// the mod author's own future fixes) and the `pack.mcmeta` format rules stay
/// in the one place that owns and tests them.
///
/// `None` for the same reasons `pack::build` returns `None`: nothing to ship,
/// or no `pack.mcmeta` exists for this format.
pub fn build_bundle(
    stores: &[crate::l10n::store::NamespaceStore],
    lang: &str,
    fmt: crate::l10n::pack_format::PackFormat,
    note: &str,
) -> Option<Vec<u8>> {
    use std::io::Write;

    let pack =
        crate::l10n::pack::build(stores, lang, fmt, &format!("Lucerna translations ({lang})"))?;

    let mut namespaces = serde_json::Map::new();
    for s in stores {
        if s.entries.is_empty() {
            continue;
        }
        let mut m = serde_json::Map::new();
        for (k, e) in &s.entries {
            m.insert(
                k.clone(),
                serde_json::json!({
                    "value": e.value,
                    "source_en": e.source_en,
                    "origin": match e.origin {
                        Origin::Manual => "manual",
                        Origin::Machine => "machine",
                    },
                }),
            );
        }
        namespaces.insert(s.namespace.clone(), serde_json::Value::Object(m));
    }
    let metadata = serde_json::json!({
        "schema": SCHEMA_VERSION,
        "lang": lang,
        "note": truncate_chars(note, NOTE_DISPLAY_CHARS),
        "namespaces": namespaces,
    });

    // Append to the finished pack rather than rebuilding the archive: the
    // entries `pack::build` wrote — and their fixed 1980 timestamps, which the
    // outdated-pack byte comparison elsewhere depends on — are preserved
    // exactly as that module produced them.
    let mut writer = zip::ZipWriter::new_append(std::io::Cursor::new(pack)).ok()?;
    let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer.start_file(METADATA_NAME, opts).ok()?;
    writer
        .write_all(serde_json::to_string_pretty(&metadata).ok()?.as_bytes())
        .ok()?;
    Some(writer.finish().ok()?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle_json(lang: &str, entries: &str) -> String {
        format!(
            r#"{{"schema":1,"lang":"{lang}","note":"from a friend","namespaces":{{"create":{{{entries}}}}}}}"#
        )
    }

    fn zip_with_metadata(meta: &str) -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
            w.start_file(METADATA_NAME, opts).unwrap();
            w.write_all(meta.as_bytes()).unwrap();
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn parses_the_spec_example_shape() {
        let meta = bundle_json(
            "uk_ua",
            r#""item.create.wrench":{"value":"Гайковий ключ","source_en":"Wrench","origin":"manual"}"#,
        );
        let parsed = parse_bundle_bytes(&zip_with_metadata(&meta)).unwrap();
        assert_eq!(parsed.lang, "uk_ua");
        assert_eq!(parsed.note, "from a friend");
        let e = &parsed.namespaces["create"]["item.create.wrench"];
        assert_eq!(e.value, "Гайковий ключ");
        assert_eq!(e.source_en, "Wrench");
        assert_eq!(e.origin, crate::l10n::store::Origin::Manual);
        assert_eq!(parsed.invalid, 0);
    }

    #[test]
    fn unknown_fields_are_ignored_missing_schema_is_fatal() {
        let ok = r#"{"schema":1,"lang":"ru_ru","future_field":true,"namespaces":{}}"#;
        assert!(parse_bundle_bytes(&zip_with_metadata(ok)).is_ok());
        let no_schema = r#"{"lang":"ru_ru","namespaces":{}}"#;
        assert_eq!(
            parse_bundle_bytes(&zip_with_metadata(no_schema)).unwrap_err(),
            BundleError::SchemaInvalid
        );
        let too_new = r#"{"schema":99,"lang":"ru_ru","namespaces":{}}"#;
        assert_eq!(
            parse_bundle_bytes(&zip_with_metadata(too_new)).unwrap_err(),
            BundleError::SchemaTooNew { found: 99 }
        );
    }

    #[test]
    fn lang_must_be_a_lowercase_mc_identifier_after_normalisation() {
        let upper = r#"{"schema":1,"lang":"RU_RU","namespaces":{}}"#;
        assert_eq!(
            parse_bundle_bytes(&zip_with_metadata(upper)).unwrap().lang,
            "ru_ru"
        );
        // ':' would become an NTFS alternate data stream in the pack filename.
        let ads = r#"{"schema":1,"lang":"ru:ru","namespaces":{}}"#;
        assert_eq!(
            parse_bundle_bytes(&zip_with_metadata(ads)).unwrap_err(),
            BundleError::LangInvalid {
                lang: "ru:ru".into()
            }
        );
    }

    #[test]
    fn invalid_namespace_is_skipped_and_counted_not_fatal() {
        // Uppercase namespace = invalid resource location: the game would
        // silently never load assets/MyMod/… — skip it rather than import a lie.
        let meta = r#"{"schema":1,"lang":"ru_ru","namespaces":{"MyMod":{"a.b":{"value":"x","source_en":"y","origin":"manual"}},"create":{"a.c":{"value":"x","source_en":"y","origin":"manual"}}}}"#;
        let parsed = parse_bundle_bytes(&zip_with_metadata(meta)).unwrap();
        assert!(!parsed.namespaces.contains_key("MyMod"));
        assert!(parsed.namespaces.contains_key("create"));
        assert_eq!(parsed.invalid, 1);
    }

    #[test]
    fn entry_validation_rules() {
        let entries = concat!(
            // newline and tab are LEGAL in a value — the store holds them today
            r#""ok.multiline":{"value":"a\nb\tc","source_en":"s","origin":"manual"},"#,
            // other C0 controls are not
            r#""bad.control":{"value":"a\u0007b","source_en":"s","origin":"manual"},"#,
            // format-specifier mismatch against the file's own source_en
            r#""bad.format":{"value":"%2$s","source_en":"%s","origin":"manual"},"#,
            // unknown origin: entry skipped, file survives
            r#""bad.origin":{"value":"v","source_en":"s","origin":"telepathy"},"#,
            // missing origin defaults to manual
            r#""ok.noorigin":{"value":"v","source_en":"s"}"#
        );
        let parsed =
            parse_bundle_bytes(&zip_with_metadata(&bundle_json("ru_ru", entries))).unwrap();
        let ns = &parsed.namespaces["create"];
        assert!(ns.contains_key("ok.multiline"));
        assert!(ns.contains_key("ok.noorigin"));
        assert_eq!(ns["ok.noorigin"].origin, crate::l10n::store::Origin::Manual);
        assert!(!ns.contains_key("bad.control"));
        assert!(!ns.contains_key("bad.format"));
        assert!(!ns.contains_key("bad.origin"));
        assert_eq!(parsed.invalid, 3);
    }

    #[test]
    fn hostile_keys_are_skipped() {
        let entries = concat!(
            r#""":{"value":"v","source_en":"s","origin":"manual"},"#,
            // U+202E RIGHT-TO-LEFT OVERRIDE spoofs the key table
            "\"a\\u202Eb\":{\"value\":\"v\",\"source_en\":\"s\",\"origin\":\"manual\"},",
            r#""fine.key":{"value":"v","source_en":"s","origin":"manual"}"#
        );
        let parsed =
            parse_bundle_bytes(&zip_with_metadata(&bundle_json("ru_ru", entries))).unwrap();
        assert_eq!(parsed.namespaces["create"].len(), 1);
        assert_eq!(parsed.invalid, 2);
    }

    #[test]
    fn not_a_bundle_errors_distinguish_a_plain_resource_pack() {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
            w.start_file("pack.mcmeta", opts).unwrap();
            w.write_all(br#"{"pack":{"pack_format":15,"description":"x"}}"#)
                .unwrap();
            w.finish().unwrap();
        }
        assert_eq!(
            parse_bundle_bytes(&buf.into_inner()).unwrap_err(),
            BundleError::NoMetadata {
                looks_like_resource_pack: true
            }
        );
        assert_eq!(
            parse_bundle_bytes(b"not a zip at all").unwrap_err(),
            BundleError::NotAZip
        );
    }

    #[test]
    fn oversized_metadata_is_rejected_by_the_bounded_read() {
        let huge = format!(
            r#"{{"schema":1,"lang":"ru_ru","note":"{}","namespaces":{{}}}}"#,
            "x".repeat(MAX_METADATA_BYTES as usize + 16)
        );
        assert_eq!(
            parse_bundle_bytes(&zip_with_metadata(&huge)).unwrap_err(),
            BundleError::MetadataTooLarge
        );
    }

    fn seeded_store(dir: &std::path::Path) {
        let mut s = crate::l10n::store::NamespaceStore::new("create", "ru_ru");
        s.set("kept.key", "моё", "mine-en", 111.0);
        s.set("same.key", "одинаково", "old-en", 111.0);
        crate::l10n::store::save(dir, &s).unwrap();
    }

    fn parsed(entries: &[(&str, &str, &str)]) -> ParsedBundle {
        let mut ns = std::collections::BTreeMap::new();
        let mut m = std::collections::BTreeMap::new();
        for (k, v, src) in entries {
            m.insert(
                k.to_string(),
                ParsedEntry {
                    value: v.to_string(),
                    source_en: src.to_string(),
                    origin: Origin::Machine,
                },
            );
        }
        ns.insert("create".to_string(), m);
        ParsedBundle {
            lang: "ru_ru".into(),
            note: String::new(),
            namespaces: ns,
            invalid: 0,
        }
    }

    #[test]
    fn inspect_counts_new_and_conflicts_and_ignores_equal_values() {
        let dir = tempfile::tempdir().unwrap();
        seeded_store(dir.path());
        let b = parsed(&[
            ("fresh.key", "новое", "new-en"),      // new
            ("kept.key", "чужое", "their-en"),     // conflict (value differs)
            ("same.key", "одинаково", "newer-en"), // equal value ⇒ neither
        ]);
        let s = inspect(dir.path(), &b);
        assert_eq!((s.new_entries, s.conflicts, s.invalid), (1, 1, 0));
        assert_eq!(s.lang, "ru_ru");
        assert_eq!(s.namespaces, vec!["create".to_string()]);
    }

    #[test]
    fn merge_keep_mine_only_fills_gaps() {
        let dir = tempfile::tempdir().unwrap();
        seeded_store(dir.path());
        let b = parsed(&[
            ("fresh.key", "новое", "new-en"),
            ("kept.key", "чужое", "their-en"),
        ]);
        let r = merge(dir.path(), &b, ConflictPolicy::KeepMine, 999.0);
        assert_eq!((r.added, r.replaced, r.kept), (1, 0, 1));

        let store = crate::l10n::store::load(dir.path(), "ru_ru", "create");
        assert_eq!(store.entries["kept.key"].value, "моё");
        assert_eq!(
            store.entries["kept.key"].updated_at, 111.0,
            "a kept entry must be untouched, timestamp included"
        );
        let fresh = &store.entries["fresh.key"];
        assert_eq!(fresh.value, "новое");
        assert_eq!(fresh.source_en, "new-en");
        assert_eq!(
            fresh.origin,
            Origin::Machine,
            "origin travels with the entry"
        );
        assert_eq!(fresh.updated_at, 999.0, "import stamps its own time");
    }

    #[test]
    fn merge_take_file_replaces_value_source_and_origin_together() {
        let dir = tempfile::tempdir().unwrap();
        seeded_store(dir.path());
        let b = parsed(&[("kept.key", "чужое", "their-en")]);
        let r = merge(dir.path(), &b, ConflictPolicy::TakeFile, 999.0);
        assert_eq!((r.added, r.replaced, r.kept), (0, 1, 0));

        let e = &crate::l10n::store::load(dir.path(), "ru_ru", "create").entries["kept.key"];
        assert_eq!(
            (e.value.as_str(), e.source_en.as_str()),
            ("чужое", "their-en")
        );
        assert_eq!(e.origin, Origin::Machine);
        assert_eq!(e.updated_at, 999.0);
    }

    #[test]
    fn an_equal_value_is_untouched_even_when_the_file_has_a_newer_source_en() {
        // The deliberate rule: staleness keeps describing what the RECIPIENT
        // translated against. Adopting the sender's newer `source_en` would
        // flip this key from stale to ok without changing a visible character.
        let dir = tempfile::tempdir().unwrap();
        seeded_store(dir.path());
        let b = parsed(&[("same.key", "одинаково", "newer-en")]);
        let r = merge(dir.path(), &b, ConflictPolicy::TakeFile, 999.0);
        assert_eq!((r.added, r.replaced, r.kept), (0, 0, 0));

        let e = &crate::l10n::store::load(dir.path(), "ru_ru", "create").entries["same.key"];
        assert_eq!(e.source_en, "old-en");
        assert_eq!(e.updated_at, 111.0);
    }

    #[test]
    fn merge_creates_a_namespace_the_recipient_never_had() {
        let dir = tempfile::tempdir().unwrap();
        let mut ns = std::collections::BTreeMap::new();
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            "a.b".to_string(),
            ParsedEntry {
                value: "v".into(),
                source_en: "V".into(),
                origin: Origin::Manual,
            },
        );
        ns.insert("brandnew".to_string(), m);
        let b = ParsedBundle {
            lang: "uk_ua".into(),
            note: String::new(),
            namespaces: ns,
            invalid: 2,
        };

        let r = merge(dir.path(), &b, ConflictPolicy::KeepMine, 5.0);
        assert_eq!((r.added, r.replaced, r.kept), (1, 0, 0));
        assert_eq!(
            r.invalid, 2,
            "the parse-time invalid count is carried through"
        );
        assert_eq!(r.lang, "uk_ua");
        assert_eq!(
            crate::l10n::store::load(dir.path(), "uk_ua", "brandnew").entries["a.b"].value,
            "v"
        );
    }

    #[test]
    fn export_filename_never_matches_the_sweep_prefix() {
        // The Apply sweep deletes every resourcepacks/ file starting with
        // PACK_PREFIX, and the no-Lucerna flow tells the recipient to drop this
        // bundle into exactly that directory. One letter — the plural 's' —
        // keeps them disjoint. Pin it: harmonising the two names would make the
        // recipient's next Apply silently delete the file a friend sent them.
        let name = default_filename("ru_ru", "2026-08-04");
        assert_eq!(name, "lucerna-translations-ru_ru-2026-08-04.zip");
        assert!(!name.starts_with(crate::l10n::options_txt::PACK_PREFIX));
        assert!(dest_filename_reserved("lucerna-translation-ru_ru.zip"));
        assert!(!dest_filename_reserved(&name));
    }

    #[test]
    fn build_bundle_is_a_real_pack_that_round_trips_through_our_own_parser() {
        let mut s = crate::l10n::store::NamespaceStore::new("create", "ru_ru");
        s.set("item.create.wrench", "Ключ", "Wrench", 1.0);
        let fmt = crate::l10n::pack_format::PackFormat {
            major: 34,
            minor: 0,
        };
        let bytes = build_bundle(&[s], "ru_ru", fmt, "от меня").expect("has overrides");

        // Still a resource pack the game can load …
        let mut ar = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        assert!(ar.by_name("pack.mcmeta").is_ok());
        assert!(ar.by_name("assets/create/lang/ru_ru.json").is_ok());
        assert!(ar.by_name(METADATA_NAME).is_ok());

        // … and a bundle our own importer reads back with full fidelity.
        let parsed = parse_bundle_bytes(&bytes).unwrap();
        assert_eq!(parsed.lang, "ru_ru");
        assert_eq!(parsed.note, "от меня");
        assert_eq!(parsed.invalid, 0);
        let e = &parsed.namespaces["create"]["item.create.wrench"];
        assert_eq!((e.value.as_str(), e.source_en.as_str()), ("Ключ", "Wrench"));
        assert_eq!(e.origin, Origin::Manual);
    }

    #[test]
    fn build_bundle_preserves_machine_origin_across_the_round_trip() {
        // `origin` travels so the recipient's editor can still tell which
        // strings a model wrote — the AI pre-fill feature's whole audit trail.
        let mut s = crate::l10n::store::NamespaceStore::new("ae2", "ru_ru");
        s.set_with_origin("k", "v", "V", 1.0, Origin::Machine);
        let fmt = crate::l10n::pack_format::PackFormat {
            major: 34,
            minor: 0,
        };
        let bytes = build_bundle(&[s], "ru_ru", fmt, "").expect("has overrides");
        let parsed = parse_bundle_bytes(&bytes).unwrap();
        assert_eq!(parsed.namespaces["ae2"]["k"].origin, Origin::Machine);
    }

    #[test]
    fn build_bundle_is_none_when_there_is_nothing_to_ship() {
        let fmt = crate::l10n::pack_format::PackFormat {
            major: 34,
            minor: 0,
        };
        assert!(build_bundle(&[], "ru_ru", fmt, "").is_none());
        // A store with no entries is equally nothing to ship.
        let empty = crate::l10n::store::NamespaceStore::new("create", "ru_ru");
        assert!(build_bundle(&[empty], "ru_ru", fmt, "").is_none());
    }
}

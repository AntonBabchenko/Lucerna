//! Assembling a user's translation overrides into a resource pack the game
//! will load.
//!
//! The pack MUST be a sparse overlay: `ClientLanguage.loadFrom` merges every
//! pack's contribution into ONE `HashMap` per key, and `FallbackResourceManager`
//! walks the pack stack lowest-priority-first — so the highest-priority pack
//! wins only the keys it actually DEFINES, while every key it omits still
//! resolves from a lower-priority pack (the mod's own file). A full copy of a
//! mod's lang file would therefore freeze every duplicated string at
//! authoring time and silently suppress the mod author's later fixes; only
//! the keys the user actually overrode may ever appear here.
//!
//! Overrides land in `assets/<namespace>/lang/<code>.json` — the SAME
//! namespace as the mod's own file, never `assets/minecraft/`. Translation
//! keys are not namespaced, and the loader's outer loop walks
//! `resourceManager.getNamespaces()` (a plain `HashMap` key set), so a key
//! defined under two different namespaces would resolve by arbitrary hash
//! iteration order, not by pack priority — writing to the wrong namespace
//! would be a coin flip, not an override.
//!
//! Only the modern JSON shape is ever emitted: applying at all requires
//! `pack_format::supports_apply` (1.13+), because on FML/Forge ≤ 1.12.2 mod
//! domains are iterated LAST, so a mod's own lang file overwrites the
//! resource pack's rather than the other way round (MinecraftForge #4907,
//! closed stale, never fixed). There is deliberately no properties-file
//! renderer here — it would be code for a path the caller can never reach.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::l10n::pack_format::{self, PackFormat};
use crate::l10n::scan;
use crate::l10n::store::NamespaceStore;

/// Assemble the pack. `None` when there is nothing to ship or nothing SAFE to
/// ship: no namespace has an override, or the format is unknown (there is no
/// `pack.mcmeta` that loads on both sides of the 1.21.9 scheme change). The
/// caller removes any previously generated pack in that case.
///
/// Contents: `pack.mcmeta` from [`pack_format::pack_mcmeta`], plus one
/// `assets/<namespace>/lang/<code>.json` per namespace with at least one
/// override, containing ONLY the overridden keys — see the module docs for
/// why a sparse overlay is the only safe shape.
///
/// `stores` need not arrive pre-sorted or pre-deduplicated by namespace:
/// entries are gathered into a `BTreeMap` keyed by namespace before anything
/// is written, so the archive's entry order — and therefore its bytes —
/// depends only on WHICH namespaces carry overrides, never on the order
/// `stores` happens to arrive in.
pub fn build(
    stores: &[NamespaceStore],
    code: &str,
    fmt: PackFormat,
    description: &str,
) -> Option<Vec<u8>> {
    let mcmeta = pack_format::pack_mcmeta(fmt, description)?;

    // Defence in depth. `namespace` reaches this function by being read back
    // off disk (`store::load`), where nothing re-validates it — unlike the
    // jar-scan boundary in `scan::parse_lang_path`, this call site was never
    // SUPPOSED to see an untrusted namespace, but a hand-edited or corrupted
    // store file can still hand back a value like ".." or one containing a
    // backslash, and this function is about to compose it straight into a
    // NEW archive entry name (`assets/<namespace>/lang/<code>.json`). Reuse
    // the SAME rule the jar boundary already enforces (`store::store_path`'s
    // percent-encoding is a filesystem-only defence and does not help an
    // archive entry name) rather than trusting the value a second time with
    // no check at all — one rule, one implementation. A namespace that fails
    // the check is DROPPED, not treated as a reason to abandon the whole
    // pack: mirrors `store::namespaces_with_overrides`, which skips one
    // malformed file rather than failing the whole listing.
    let namespaces: BTreeMap<&str, &NamespaceStore> = stores
        .iter()
        .filter(|s| !s.entries.is_empty())
        .filter(|s| !scan::is_traversal_unsafe(&s.namespace))
        .map(|s| (s.namespace.as_str(), s))
        .collect();

    if namespaces.is_empty() {
        return None;
    }

    let mut cursor = Cursor::new(Vec::new());
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    {
        let mut zw = ZipWriter::new(&mut cursor);

        zw.start_file("pack.mcmeta", options).ok()?;
        zw.write_all(mcmeta.as_bytes()).ok()?;

        for store in namespaces.into_values() {
            // Only the KEY and its VALUE — never the rest of `Entry`
            // (`source_en`/`origin`/`updated_at` are staleness-tracking
            // metadata for the editor; the game has no use for them and must
            // never see them).
            let lang: BTreeMap<&str, &str> = store
                .entries
                .iter()
                .map(|(key, entry)| (key.as_str(), entry.value.as_str()))
                .collect();
            // Serialising a `BTreeMap` walks keys in sorted order, so
            // identical overrides always render identical bytes — the
            // byte-stability an unchanged pack relies on to not churn on
            // disk.
            let body = serde_json::to_string(&lang).ok()?;

            let path = format!("assets/{}/lang/{code}.json", store.namespace);
            zw.start_file(&path, options).ok()?;
            zw.write_all(body.as_bytes()).ok()?;
        }

        zw.finish().ok()?;
    }

    Some(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `NamespaceStore` with the given overrides. `entries` is
    /// `(key, value, source_en)`; `updated_at` is a fixed constant since
    /// nothing here exercises staleness.
    fn store(namespace: &str, lang: &str, entries: &[(&str, &str, &str)]) -> NamespaceStore {
        let mut s = NamespaceStore::new(namespace, lang);
        for (key, value, source_en) in entries {
            s.set(*key, *value, *source_en, 1.0);
        }
        s
    }

    /// Parse `bytes` as a zip and return every entry name, sorted.
    fn entry_names(bytes: &[u8]) -> Vec<String> {
        let zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("valid zip");
        let mut names: Vec<String> = zip.file_names().map(String::from).collect();
        names.sort();
        names
    }

    // -----------------------------------------------------------------
    // sparse content
    // -----------------------------------------------------------------

    #[test]
    fn lang_file_contains_only_the_overridden_keys() {
        let s = store(
            "create",
            "ru_ru",
            &[
                ("item.create.wrench", "Гаечный ключ", "Wrench"),
                ("gui.create.empty", "Пусто", ""),
            ],
        );
        let bytes = build(
            &[s],
            "ru_ru",
            PackFormat {
                major: 34,
                minor: 0,
            },
            "x",
        )
        .expect("has overrides, known format");

        let body = scan::read_entry(&bytes, "assets/create/lang/ru_ru.json").expect("present");
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let obj = v.as_object().expect("json object");
        // Exactly the two overridden keys — not a superset, not a subset.
        assert_eq!(obj.len(), 2);
        assert_eq!(obj["item.create.wrench"], "Гаечный ключ");
        assert_eq!(obj["gui.create.empty"], "Пусто");
    }

    #[test]
    fn override_lands_in_the_mods_own_namespace_not_minecraft() {
        let s = store("create", "ru_ru", &[("a", "А", "A")]);
        let bytes = build(
            &[s],
            "ru_ru",
            PackFormat {
                major: 34,
                minor: 0,
            },
            "x",
        )
        .expect("has overrides, known format");

        assert!(scan::read_entry(&bytes, "assets/create/lang/ru_ru.json").is_some());
        // The dangerous alternative: writing to assets/minecraft/ would let
        // the outer namespace-iteration hash-order race decide which pack
        // wins, instead of the resource-pack priority stack. Must never
        // happen.
        assert!(scan::read_entry(&bytes, "assets/minecraft/lang/ru_ru.json").is_none());
    }

    #[test]
    fn multiple_namespaces_each_get_their_own_lang_file() {
        let create = store("create", "ru_ru", &[("a", "А", "A")]);
        let thermal = store("thermal", "ru_ru", &[("b", "Б", "B")]);
        let bytes = build(
            &[create, thermal],
            "ru_ru",
            PackFormat {
                major: 34,
                minor: 0,
            },
            "x",
        )
        .expect("has overrides, known format");

        assert!(scan::read_entry(&bytes, "assets/create/lang/ru_ru.json").is_some());
        assert!(scan::read_entry(&bytes, "assets/thermal/lang/ru_ru.json").is_some());
    }

    // -----------------------------------------------------------------
    // pack.mcmeta
    // -----------------------------------------------------------------

    #[test]
    fn pack_mcmeta_is_always_present_and_carries_the_requested_format() {
        let s = store("create", "ru_ru", &[("a", "А", "A")]);
        let fmt = PackFormat {
            major: 34,
            minor: 0,
        };
        let bytes = build(&[s], "ru_ru", fmt, "My Translations").expect("known format");

        let body = scan::read_entry(&bytes, "pack.mcmeta").expect("pack.mcmeta present");
        let expected = pack_format::pack_mcmeta(fmt, "My Translations").unwrap();
        assert_eq!(body, expected.into_bytes());
    }

    #[test]
    fn pack_mcmeta_reflects_the_new_range_scheme_above_format_65() {
        let s = store("create", "ru_ru", &[("a", "А", "A")]);
        let fmt = PackFormat {
            major: 75,
            minor: 0,
        };
        let bytes = build(&[s], "ru_ru", fmt, "x").expect("known format");

        let body = scan::read_entry(&bytes, "pack.mcmeta").expect("pack.mcmeta present");
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["pack"]["min_format"], serde_json::json!([75, 0]));
        assert!(v["pack"].get("pack_format").is_none());
    }

    // -----------------------------------------------------------------
    // skipping / None cases
    // -----------------------------------------------------------------

    #[test]
    fn a_namespace_with_no_overrides_is_skipped() {
        let empty = store("empty_mod", "ru_ru", &[]);
        let create = store("create", "ru_ru", &[("a", "А", "A")]);
        let bytes = build(
            &[empty, create],
            "ru_ru",
            PackFormat {
                major: 34,
                minor: 0,
            },
            "x",
        )
        .expect("create has overrides");

        assert!(scan::read_entry(&bytes, "assets/create/lang/ru_ru.json").is_some());
        assert!(scan::read_entry(&bytes, "assets/empty_mod/lang/ru_ru.json").is_none());
    }

    #[test]
    fn an_all_empty_store_set_yields_none() {
        let stores = [
            store("create", "ru_ru", &[]),
            store("thermal", "ru_ru", &[]),
        ];
        assert!(build(
            &stores,
            "ru_ru",
            PackFormat {
                major: 34,
                minor: 0
            },
            "x"
        )
        .is_none());

        // The degenerate case of no stores at all must also yield None, not
        // a pack containing only pack.mcmeta.
        assert!(build(
            &[],
            "ru_ru",
            PackFormat {
                major: 34,
                minor: 0
            },
            "x"
        )
        .is_none());
    }

    #[test]
    fn unknown_format_yields_none_even_with_real_overrides() {
        let s = store("create", "ru_ru", &[("a", "А", "A")]);
        assert!(build(&[s], "ru_ru", PackFormat::UNKNOWN, "x").is_none());
    }

    // -----------------------------------------------------------------
    // namespace defence in depth
    // -----------------------------------------------------------------

    #[test]
    fn a_traversal_unsafe_namespace_is_dropped_not_fatal_to_the_pack() {
        // A namespace like ".." can only reach this function via a
        // hand-edited or corrupted store file on disk — `store::load` never
        // validates it on the way back in. It must not derail overrides that
        // legitimately exist for OTHER, safe namespaces in the same batch.
        let unsafe_store = store("..", "ru_ru", &[("a", "А", "A")]);
        let safe_store = store("create", "ru_ru", &[("b", "Б", "B")]);
        let bytes = build(
            &[unsafe_store, safe_store],
            "ru_ru",
            PackFormat {
                major: 34,
                minor: 0,
            },
            "x",
        )
        .expect("the safe namespace alone is enough to ship a pack");

        assert!(scan::read_entry(&bytes, "assets/create/lang/ru_ru.json").is_some());
        // No entry can even be named for the unsafe namespace's would-be
        // path, so absence is checked at the archive-listing level instead.
        let names = entry_names(&bytes);
        assert!(names.iter().all(|n| !n.contains("..")));
    }

    #[test]
    fn an_all_traversal_unsafe_store_set_yields_none() {
        let s = store("..", "ru_ru", &[("a", "А", "A")]);
        assert!(build(
            &[s],
            "ru_ru",
            PackFormat {
                major: 34,
                minor: 0
            },
            "x"
        )
        .is_none());
    }

    // -----------------------------------------------------------------
    // byte stability
    // -----------------------------------------------------------------

    #[test]
    fn building_twice_from_equal_input_gives_identical_bytes() {
        // Deliberately re-construct equal-but-distinct `NamespaceStore`
        // values, and insert entries in a DIFFERENT order the second time —
        // mirrors `a_saved_cache_is_byte_stable_across_rewrites` in
        // `coverage.rs`, which pins the same guarantee for `ScanCache`.
        let first = vec![
            store("create", "ru_ru", &[("a", "А", "A"), ("b", "Б", "B")]),
            store("thermal", "ru_ru", &[("c", "В", "C")]),
        ];
        let second = vec![
            store("thermal", "ru_ru", &[("c", "В", "C")]),
            store("create", "ru_ru", &[("b", "Б", "B"), ("a", "А", "A")]),
        ];

        let fmt = PackFormat {
            major: 34,
            minor: 0,
        };
        let a = build(&first, "ru_ru", fmt, "x").unwrap();
        let b = build(&second, "ru_ru", fmt, "x").unwrap();
        assert_eq!(a, b);
    }
}

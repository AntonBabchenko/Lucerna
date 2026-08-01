//! Merge one resource namespace's English and target-language lang files
//! across every enabled mod that ships it, for the per-key editor view.
//!
//! Reads lazily for a SINGLE namespace: a whole instance's key set is
//! hundreds of thousands of strings and is never materialised (see
//! `commands::l10n_namespace_keys`'s doc comment) — this only ever opens the
//! jars that ship the one namespace the caller asked about, and only reads
//! their `en_us` and target-language lang entries.
//!
//! # Merge precedence across jars
//!
//! A namespace can be split across more than one jar — Jar-in-Jar, or two
//! unrelated mods that happen to reuse the same namespace string — and the
//! game merges their lang files per key into one shared map (`l10n::coverage`'s
//! module doc: "the game loads en_us into the shared map first"). Precedence
//! between two jars supplying the SAME key for the SAME namespace is not
//! something this launcher can observe or reproduce — it depends on
//! Forge/Fabric's own mod-loading order, which this scanner never sees. We
//! resolve it by iterating `mods::installed::list`'s order and letting a
//! LATER jar's value win (`LangMap::extend`'s natural behaviour), which is
//! deterministic and good enough for an editor. `l10n::coverage::merge_into`
//! accepts the identical imprecision for per-namespace COUNTS, for the same
//! underlying reason: the game's true precedence is unobservable from here.

use std::path::Path;

use crate::error::Error;
use crate::l10n::scan::{self, LangMap};

/// `(english, target-language)` lang maps for `namespace`, merged across
/// every ENABLED mod jar that ships it. Either map may be empty — no jar
/// ships English for this namespace, or none ships the target language —
/// which the caller treats the same way `state_of`/`namespace_coverage`
/// already treat an absent target file: as "not translated by the mod",
/// never as an error.
///
/// A jar that vanished since the mod list was read, or that is present but
/// unreadable (truncated download, unusual archive), is skipped rather than
/// failing the whole lookup — the same tolerance `coverage::scan_instance`
/// already extends to the identical two failure modes.
pub async fn namespace_lang_maps(
    inst_root: &Path,
    namespace: &str,
    lang: &str,
) -> Result<(LangMap, LangMap), Error> {
    let installed = crate::mods::installed::list(inst_root).await?;
    let mods_dir = crate::mods::installed::mods_dir(inst_root);

    let mut en = LangMap::new();
    let mut target = LangMap::new();

    for m in installed.iter().filter(|m| m.enabled) {
        let Ok(bytes) = tokio::fs::read(mods_dir.join(&m.filename)).await else {
            continue;
        };
        let Ok(entries) = scan::list_lang_entries(&bytes) else {
            continue;
        };
        for (entry, path) in &entries {
            if entry.namespace != namespace {
                continue;
            }
            if entry.code == "en_us" {
                if let Some(m) = scan::read_lang_map(&bytes, entry, path) {
                    en.extend(m);
                }
            } else if entry.code == lang {
                if let Some(m) = scan::read_lang_map(&bytes, entry, path) {
                    target.extend(m);
                }
            }
        }
    }

    Ok((en, target))
}

#[cfg(test)]
mod tests {
    use sha1::{Digest, Sha1};

    use super::*;

    /// Build an in-memory jar containing the given (path, contents) entries.
    /// Local copy of the identical helper in `scan.rs`'s and `coverage.rs`'s
    /// test modules — not worth making `pub` for a six-line helper only
    /// tests use.
    fn jar(entries: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            for (name, body) in entries {
                w.start_file(*name, opts).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    /// Drop `bytes` into `<inst_root>/.minecraft/mods/<filename>` and let
    /// `mods::installed::list`'s reconcile step discover it — mirrors
    /// `coverage.rs`'s test seeding (no registry JSON needed; a `.jar` file
    /// on disk with no record becomes a synthesized ENABLED entry).
    async fn seed_mod(inst_root: &Path, filename: &str, bytes: &[u8]) {
        let mods_dir = crate::mods::installed::mods_dir(inst_root);
        tokio::fs::create_dir_all(&mods_dir).await.unwrap();
        tokio::fs::write(mods_dir.join(filename), bytes)
            .await
            .unwrap();
    }

    /// Like [`seed_mod`], but goes through `mods::installed::add` first so
    /// the registry order matches the CALL order rather than whatever order
    /// the OS's directory enumeration happens to return (which reconcile's
    /// synthesis step depends on, and which is not guaranteed across
    /// platforms) — needed only by the "later jar wins" test below, where
    /// the merge order must be deterministic for the assertion to mean
    /// anything.
    async fn seed_mod_in_order(inst_root: &Path, filename: &str, bytes: &[u8]) {
        let mods_dir = crate::mods::installed::mods_dir(inst_root);
        tokio::fs::create_dir_all(&mods_dir).await.unwrap();
        tokio::fs::write(mods_dir.join(filename), bytes)
            .await
            .unwrap();
        let sha1 = hex::encode(Sha1::digest(bytes));
        crate::mods::installed::add(
            inst_root,
            crate::mods::platform::InstalledMod {
                filename: filename.to_string(),
                sha1,
                source: None,
                project_id: None,
                version_id: None,
                name: filename.to_string(),
                version_number: None,
                installed_at: "2026-01-01T00:00:00+00:00".into(),
                enabled: true,
                enrich_attempted: false,
                requires: Vec::new(),
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn merges_a_single_jars_english_and_target_maps() {
        let td = tempfile::tempdir().unwrap();
        let bytes = jar(&[
            ("assets/create/lang/en_us.json", r#"{"a":"A","b":"B"}"#),
            ("assets/create/lang/ru_ru.json", r#"{"a":"А"}"#),
        ]);
        seed_mod(td.path(), "create.jar", &bytes).await;

        let (en, target) = namespace_lang_maps(td.path(), "create", "ru_ru")
            .await
            .unwrap();
        assert_eq!(en.get("a").map(String::as_str), Some("A"));
        assert_eq!(en.get("b").map(String::as_str), Some("B"));
        assert_eq!(target.get("a").map(String::as_str), Some("А"));
        assert_eq!(target.get("b"), None);
    }

    #[tokio::test]
    async fn merges_disjoint_keys_across_two_jars_sharing_a_namespace() {
        // Jar-in-Jar: two jars both contribute to "create".
        let td = tempfile::tempdir().unwrap();
        let a = jar(&[("assets/create/lang/en_us.json", r#"{"a":"A"}"#)]);
        let b = jar(&[("assets/create/lang/en_us.json", r#"{"b":"B"}"#)]);
        seed_mod(td.path(), "create-core.jar", &a).await;
        seed_mod(td.path(), "create-addon.jar", &b).await;

        let (en, _) = namespace_lang_maps(td.path(), "create", "ru_ru")
            .await
            .unwrap();
        assert_eq!(en.len(), 2);
        assert_eq!(en.get("a").map(String::as_str), Some("A"));
        assert_eq!(en.get("b").map(String::as_str), Some("B"));
    }

    #[tokio::test]
    async fn a_later_jar_wins_a_key_both_jars_define() {
        let td = tempfile::tempdir().unwrap();
        let a = jar(&[("assets/create/lang/en_us.json", r#"{"a":"first"}"#)]);
        let b = jar(&[("assets/create/lang/en_us.json", r#"{"a":"second"}"#)]);
        // `seed_mod_in_order` registers via `mods::installed::add`, so the
        // registry order is exactly the call order below, independent of
        // directory-enumeration order.
        seed_mod_in_order(td.path(), "create-a.jar", &a).await;
        seed_mod_in_order(td.path(), "create-b.jar", &b).await;

        let (en, _) = namespace_lang_maps(td.path(), "create", "ru_ru")
            .await
            .unwrap();
        assert_eq!(en.get("a").map(String::as_str), Some("second"));
    }

    #[tokio::test]
    async fn a_different_namespace_in_the_same_jar_is_ignored() {
        let td = tempfile::tempdir().unwrap();
        let bytes = jar(&[
            ("assets/create/lang/en_us.json", r#"{"a":"A"}"#),
            ("assets/thermal/lang/en_us.json", r#"{"b":"B"}"#),
        ]);
        seed_mod(td.path(), "pack.jar", &bytes).await;

        let (en, _) = namespace_lang_maps(td.path(), "create", "ru_ru")
            .await
            .unwrap();
        assert_eq!(en.len(), 1);
        assert!(en.contains_key("a"));
    }

    #[tokio::test]
    async fn a_disabled_mod_does_not_contribute() {
        let td = tempfile::tempdir().unwrap();
        let bytes = jar(&[("assets/create/lang/en_us.json", r#"{"a":"A"}"#)]);
        // `.jar.disabled` is reconcile's on-disk marker for a disabled mod
        // (see `mods::installed::reconcile` — the same convention
        // `commands::remove_pack_file` relies on elsewhere).
        seed_mod(td.path(), "create.jar.disabled", &bytes).await;

        let (en, _) = namespace_lang_maps(td.path(), "create", "ru_ru")
            .await
            .unwrap();
        assert!(en.is_empty());
    }

    #[tokio::test]
    async fn a_corrupt_jar_is_skipped_without_failing_the_lookup() {
        let td = tempfile::tempdir().unwrap();
        seed_mod(td.path(), "broken.jar", b"not a zip at all").await;

        let (en, target) = namespace_lang_maps(td.path(), "create", "ru_ru")
            .await
            .expect("one corrupt jar must not fail the whole lookup");
        assert!(en.is_empty());
        assert!(target.is_empty());
    }

    #[tokio::test]
    async fn no_matching_namespace_at_all_yields_empty_maps() {
        let td = tempfile::tempdir().unwrap();
        let (en, target) = namespace_lang_maps(td.path(), "create", "ru_ru")
            .await
            .unwrap();
        assert!(en.is_empty());
        assert!(target.is_empty());
    }
}

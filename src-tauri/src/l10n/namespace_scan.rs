//! Merge resource namespaces' English and target-language lang files across
//! every enabled mod that ships them.
//!
//! Two entry points, for two genuinely different access patterns:
//!
//! - [`namespace_lang_maps`] answers for a SINGLE namespace, for the per-key
//!   editor view. A whole instance's key set is hundreds of thousands of
//!   strings and is never materialised for the editor (see
//!   `commands::l10n_namespace_keys`'s doc comment), so this reads only the
//!   `en_us` and target-language entries of the one namespace asked about.
//! - [`instance_lang_maps`] answers for EVERY namespace at once, in a single
//!   pass over the jars. Asking the per-namespace function N times would
//!   re-read every jar N times — O(namespaces × jars), and a 150-mod pack has
//!   comparably many namespaces — which is affordable for one editor panel and
//!   not for a whole-instance job. `coverage`'s per-jar cache cannot stand in:
//!   it stores counts, never strings.
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

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::Error;
use crate::l10n::scan::{self, LangMap};

/// One mod jar's bytes, or `None` when it vanished between the mod list and
/// here, or is present but unreadable (truncated download, unusual archive).
///
/// Both scanners funnel their disk reads through this single place, which is
/// what makes [`instance_lang_maps`]'s one-pass guarantee a MEASURED fact
/// rather than a claim: under `cfg(test)` this bumps a counter that
/// `one_pass_collects_every_namespace_with_a_single_read_per_jar` asserts on,
/// against the cost of the per-namespace function over the same fixture.
/// Compiled away entirely in a release build.
async fn read_jar(path: &Path) -> Option<Vec<u8>> {
    let bytes = tokio::fs::read(path).await.ok()?;
    #[cfg(test)]
    note_jar_read();
    Some(bytes)
}

// Jar reads performed on THIS thread since the last `take_jar_reads`.
//
// Thread-local rather than a global counter, deliberately: libtest gives each
// test its own thread and several tests in this module read jars, so a shared
// counter would be polluted by whatever happens to run alongside. Plain
// `#[tokio::test]` builds a current-thread runtime, which polls the future on
// the test's own thread, so every increment lands where the assertion can see
// it. (A future switch to the multi-thread flavour would under-count and fail
// the assertion loudly — the safe direction.)
//
// A plain comment, not a doc comment: rustdoc does not document items produced
// by a macro invocation, and `unused_doc_comments` warns on the attempt.
#[cfg(test)]
thread_local! {
    static JAR_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn note_jar_read() {
    JAR_READS.with(|c| c.set(c.get() + 1));
}

/// Read and reset this thread's jar-read counter.
#[cfg(test)]
fn take_jar_reads() -> usize {
    JAR_READS.with(|c| c.replace(0))
}

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
        let Some(bytes) = read_jar(&mods_dir.join(&m.filename)).await else {
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

/// Every namespace in the instance, with its merged English and target-language
/// maps, in a SINGLE pass over the enabled jars.
///
/// The whole-instance counterpart to [`namespace_lang_maps`], which re-reads
/// every jar for each namespace it is asked about. This walks each jar exactly
/// once and buckets its lang entries by the namespace they belong to; the
/// per-entry parsing, the legacy `.lang` format and the `en_US` → `en_us`
/// region normalisation are all the same `l10n::scan` code the single-namespace
/// path uses, so the two can never disagree about a key. Merge order is
/// likewise identical — `mods::installed::list` order, later jar wins — so the
/// caveats in this module's own doc apply unchanged.
///
/// A namespace appears in the result exactly when some enabled jar ships its
/// `en_us` or its `lang` file. One that ships neither (only `de_de`, say)
/// is absent rather than present-and-empty: there is nothing to translate and
/// nothing to translate it from, and the per-namespace function would answer
/// the same pair of empty maps for a namespace that does not exist at all.
///
/// A jar that vanished since the mod list was read, or that is unreadable, is
/// skipped rather than failing the whole pass — the same tolerance
/// [`namespace_lang_maps`] and `coverage::scan_instance` already extend.
pub async fn instance_lang_maps(
    inst_root: &Path,
    lang: &str,
) -> Result<BTreeMap<String, (LangMap, LangMap)>, Error> {
    let installed = crate::mods::installed::list(inst_root).await?;
    let mods_dir = crate::mods::installed::mods_dir(inst_root);

    let mut out: BTreeMap<String, (LangMap, LangMap)> = BTreeMap::new();

    for m in installed.iter().filter(|m| m.enabled) {
        let Some(bytes) = read_jar(&mods_dir.join(&m.filename)).await else {
            continue;
        };
        let Ok(entries) = scan::list_lang_entries(&bytes) else {
            continue;
        };
        for (entry, path) in &entries {
            // The same two-arm dispatch as above, in the same order: when
            // `lang` IS `en_us` the entry counts as English and the target map
            // stays empty, exactly as the `else if` there leaves it.
            let english = entry.code == "en_us";
            if !english && entry.code != lang {
                continue;
            }
            let Some(map) = scan::read_lang_map(&bytes, entry, path) else {
                continue;
            };
            let slot = out.entry(entry.namespace.clone()).or_default();
            if english {
                slot.0.extend(map);
            } else {
                slot.1.extend(map);
            }
        }
    }

    Ok(out)
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

    // ---------------------------------------------------------------------
    // instance_lang_maps — the one-pass whole-instance collector
    // ---------------------------------------------------------------------

    #[tokio::test]
    async fn one_pass_collects_every_namespace_with_a_single_read_per_jar() {
        // Two ENABLED jars carrying three namespaces between them, plus a
        // disabled jar carrying a fourth. The property under test is the
        // whole reason this function exists: the per-namespace API re-reads
        // every enabled jar once PER NAMESPACE, so answering for three
        // namespaces costs six jar reads; the one-pass API must cost two.
        // `take_jar_reads` makes that a measured number rather than a claim —
        // an implementation that merely loops over `namespace_lang_maps`
        // internally would return an identical map and fail only here.
        let td = tempfile::tempdir().unwrap();
        let a = jar(&[
            (
                "assets/create/lang/en_us.json",
                r#"{"item.create.a":"Cogwheel"}"#,
            ),
            (
                "assets/create/lang/ru_ru.json",
                r#"{"item.create.a":"Шестерня"}"#,
            ),
            (
                "assets/thermal/lang/en_us.json",
                r#"{"item.th.a":"Dynamo"}"#,
            ),
        ]);
        let b = jar(&[
            // Same namespace as jar A, disjoint keys: the one-pass merge must
            // union them exactly as the per-namespace API does.
            (
                "assets/create/lang/en_us.json",
                r#"{"item.create.b":"Andesite Alloy"}"#,
            ),
            // Legacy `.lang` with an uppercase region — must be recognised
            // through the module's existing entry handling, not a second copy
            // of it.
            ("assets/ae2/lang/en_US.lang", "item.ae2.a=Certus Quartz"),
        ]);
        // A fourth namespace behind a disabled jar: it must neither appear in
        // the result nor cost a read.
        let c = jar(&[("assets/tconstruct/lang/en_us.json", r#"{"item.tc.a":"X"}"#)]);
        seed_mod(td.path(), "create.jar", &a).await;
        seed_mod(td.path(), "addon.jar", &b).await;
        seed_mod(td.path(), "tconstruct.jar.disabled", &c).await;

        take_jar_reads(); // discard whatever the seeding above cost
        let all = instance_lang_maps(td.path(), "ru_ru").await.expect("scan");
        let one_pass_reads = take_jar_reads();

        assert_eq!(
            all.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["ae2", "create", "thermal"],
            "one call must cover every namespace of every enabled jar, and only those"
        );
        let (en, tr) = all.get("create").expect("create present");
        assert_eq!(
            en.get("item.create.a").map(String::as_str),
            Some("Cogwheel")
        );
        assert_eq!(
            en.get("item.create.b").map(String::as_str),
            Some("Andesite Alloy"),
            "a namespace split across two jars must be merged, not overwritten"
        );
        assert_eq!(
            tr.get("item.create.a").map(String::as_str),
            Some("Шестерня")
        );
        assert_eq!(
            all.get("ae2").expect("ae2 present").0.get("item.ae2.a"),
            Some(&"Certus Quartz".to_string()),
            "legacy .lang with an uppercase region must be handled by the existing entry logic"
        );

        assert_eq!(
            one_pass_reads, 2,
            "two enabled jars must cost exactly two jar reads no matter how \
             many namespaces they carry (the disabled jar costs none)"
        );

        // The contrast, measured on the same fixture: answering the same
        // three questions through the per-namespace API costs 3 x 2 reads.
        // Each answer must also be byte-identical, so the two functions can
        // never disagree about a key.
        for ns in ["ae2", "create", "thermal"] {
            let pair = namespace_lang_maps(td.path(), ns, "ru_ru")
                .await
                .expect("scan");
            assert_eq!(
                &pair,
                all.get(ns).expect("namespace present"),
                "one-pass must agree with the per-namespace API for {ns}"
            );
        }
        assert_eq!(
            take_jar_reads(),
            6,
            "sanity-check on the counter itself: the per-namespace API is the \
             quadratic one this function exists to replace"
        );
    }

    #[tokio::test]
    async fn one_pass_over_an_instance_with_no_mods_is_empty_not_an_error() {
        let td = tempfile::tempdir().unwrap();
        let all = instance_lang_maps(td.path(), "ru_ru").await.expect("scan");
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn one_pass_skips_a_corrupt_jar_and_keeps_the_rest() {
        let td = tempfile::tempdir().unwrap();
        seed_mod(td.path(), "broken.jar", b"not a zip at all").await;
        seed_mod(
            td.path(),
            "create.jar",
            &jar(&[("assets/create/lang/en_us.json", r#"{"a":"A"}"#)]),
        )
        .await;

        let all = instance_lang_maps(td.path(), "ru_ru")
            .await
            .expect("one corrupt jar must not fail the whole pass");
        assert_eq!(all.len(), 1);
        assert!(all.contains_key("create"));
    }
}

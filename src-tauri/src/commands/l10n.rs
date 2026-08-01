use super::*;

// =========================================================================
// In-game mod localization — translation coverage
// =========================================================================

pub use crate::l10n::coverage::InstanceCoverage;
pub use crate::l10n::store::KeyRow;

/// Scan the instance's enabled mods for language coverage in `lang`.
///
/// Per-jar results are cached by (language, jar SHA-1), so an unchanged
/// instance re-renders with zero jar reads. An empty `lang` means "derive from
/// the UI locale": the persisted `GeneralSettings.language` from `app.json`,
/// mapped through `default_target_code`. A non-empty `lang` is normalized
/// through `default_target_code` too, so a caller may pass either a bare
/// launcher locale (`"ru"`) or a full Minecraft code (`"ru_ru"`) — the
/// function is idempotent for the latter.
#[tauri::command]
#[specta::specta]
pub async fn l10n_coverage(
    app: tauri::AppHandle,
    instance_id: String,
    lang: String,
) -> Result<InstanceCoverage, crate::error::Error> {
    let inst_root = instance_root(&app, &instance_id)?;
    let cache_path = crate::paths::l10n_scan_cache_file(&app)
        .map_err(|e| crate::error::Error::io("<l10n_scan_cache_file>", e))?;
    let store_dir =
        crate::paths::l10n_dir(&app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let versions_dir = crate::paths::versions_dir(&app)
        .map_err(|e| crate::error::Error::io("<versions_dir>", e))?;
    let lang = if lang.is_empty() {
        ui_locale_target_code(&app)
    } else {
        crate::l10n::coverage::default_target_code(&lang)
    };
    // `apply_gate` needs the instance's raw MC version to locate its pristine
    // client jar; a missing/unreadable instance.json degrades to an empty
    // version rather than failing the whole coverage scan — `scan_instance`
    // then reports `ApplyGate::UnknownFormat`, which is the correct answer
    // for "we don't know this instance's version" too.
    let mc_version = read_active_mc_and_loader(&app, &instance_id)
        .map(|(mc, _loader)| mc)
        .unwrap_or_default();
    crate::l10n::coverage::scan_instance(
        &inst_root,
        &cache_path,
        &store_dir,
        &versions_dir,
        &mc_version,
        &lang,
    )
    .await
}

/// Resolve the launcher's UI locale to a Minecraft target code, for the
/// empty-`lang` "derive from the UI locale" case.
///
/// A settings read failure here — missing `app.json` on a fresh install, a
/// corrupt file, an unreadable data root — must not fail the coverage scan:
/// the whole point of this fallback is to pick a REASONABLE default language,
/// and the language picker in the UI lets the user override it either way.
/// `"en_us"` is always a safe value to fall back to.
fn ui_locale_target_code(app: &tauri::AppHandle) -> String {
    let Ok(path) = crate::paths::app_file(app) else {
        return "en_us".to_string();
    };
    let Ok(settings) = crate::instances::store::read_app_json(&path) else {
        return "en_us".to_string();
    };
    resolve_ui_language(&settings.general.language)
}

/// Map the raw `GeneralSettings.language` value (`"system"`, or a BCP-47-ish
/// code such as `"ru"` / `"pt-BR"`) to a Minecraft target code. Split out as a
/// pure function — unlike the app.json read around it — so the `"system"`
/// special-case is unit-testable without a Tauri handle.
fn resolve_ui_language(language: &str) -> String {
    if language == "system" {
        return "en_us".to_string();
    }
    crate::l10n::coverage::default_target_code(language)
}

// =========================================================================
// In-game mod localization — the override editor
// =========================================================================

/// Every key of one namespace, with its state, for the editor's key table.
///
/// Read LAZILY for a single namespace: an instance's full key set across
/// every namespace is hundreds of thousands of strings and is never
/// materialised — this only opens the jars that ship `namespace`, via
/// `l10n::namespace_scan::namespace_lang_maps`. The English file (merged
/// across every enabled jar shipping this namespace) is the key universe;
/// an override for a key the mod no longer ships (an orphan) is appended
/// explicitly by `l10n::store::namespace_key_rows`, or the user could never
/// find and clear it.
#[tauri::command]
#[specta::specta]
pub async fn l10n_namespace_keys(
    app: tauri::AppHandle,
    instance_id: String,
    namespace: String,
    lang: String,
) -> Result<Vec<KeyRow>, crate::error::Error> {
    let inst_root = instance_root(&app, &instance_id)?;
    let store_dir =
        crate::paths::l10n_dir(&app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let (en, mod_tr) =
        crate::l10n::namespace_scan::namespace_lang_maps(&inst_root, &namespace, &lang).await?;
    let store = crate::l10n::store::load(&store_dir, &lang, &namespace);
    Ok(crate::l10n::store::namespace_key_rows(
        &store,
        &en,
        Some(&mod_tr),
    ))
}

/// Reject a `namespace` or `lang` that would corrupt a later composed
/// zip-entry path if persisted verbatim: `l10n::store::load` (the very next
/// thing `l10n_set_override` calls) constructs a fresh `NamespaceStore` from
/// these two values verbatim when no file exists yet for the pair, and
/// `store::save` then persists them INSIDE the JSON body — `store_path`'s
/// percent-encoding only sanitises the FILE NAME the store lands at, never
/// that stored field. Without this check, a namespace like `"../../evil"`
/// would be silently written to disk and only dropped later, when a pack is
/// actually built (`pack::build`'s own defence-in-depth guard). Reuses
/// `l10n::scan::is_traversal_unsafe` — the identical rule `pack::build`
/// already applies to the same class of value read back off disk — rather
/// than inventing a second one. Split out as its own pure function (mirrors
/// `apply_write_allowed` above) purely so the decision is unit-testable
/// without a Tauri `AppHandle`.
fn validate_override_identifiers(namespace: &str, lang: &str) -> Result<(), crate::error::Error> {
    if crate::l10n::scan::is_traversal_unsafe(namespace) {
        return Err(crate::error::Error::L10nNamespaceInvalid {
            namespace: namespace.to_string(),
        });
    }
    if crate::l10n::scan::is_traversal_unsafe(lang) {
        return Err(crate::error::Error::L10nLangInvalid {
            lang: lang.to_string(),
        });
    }
    Ok(())
}

/// Write or clear one translation override in the global per-`(lang,
/// namespace)` store.
///
/// An empty `value` CLEARS the override rather than writing an empty
/// string — Minecraft lang values are legitimately allowed to be empty
/// (`gui.create.empty`-style keys), so an explicit clear needs a distinct
/// signal from "the user wants an empty translation"; the editor UI is
/// expected to offer a separate "Clear" action that calls this with `""`
/// rather than letting a user accidentally blank a real value into a clear.
///
/// Validation happens HERE, before anything reaches the store: `source_en`
/// is the English string the caller already has (from the `KeyRow` it
/// fetched via `l10n_namespace_keys`), so no extra read is needed to check
/// `value` against Minecraft's `%s`/`%N$s` format grammar. `namespace` and
/// `lang` are validated too, via `validate_override_identifiers` — see its
/// doc comment for why this specific boundary is the one that matters.
#[tauri::command]
#[specta::specta]
pub fn l10n_set_override(
    app: tauri::AppHandle,
    namespace: String,
    lang: String,
    key: String,
    value: String,
    source_en: String,
) -> Result<(), crate::error::Error> {
    validate_override_identifiers(&namespace, &lang)?;

    let store_dir =
        crate::paths::l10n_dir(&app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let mut store = crate::l10n::store::load(&store_dir, &lang, &namespace);

    if value.is_empty() {
        store.remove(&key);
    } else {
        crate::l10n::validate::validate(&source_en, &value).map_err(|reason| {
            crate::error::Error::L10nTranslationInvalid {
                key: key.clone(),
                reason,
            }
        })?;
        store.set(key, value, source_en, crate::instances::unix_ms_f64());
    }

    crate::l10n::store::save(&store_dir, &store)
        .map_err(|e| crate::error::Error::io("<l10n override store>", e))
}

/// Pure predicate over a snapshot the caller already has — mirrors
/// `datapacks::guard::datapack_write_allowed` and the identical internal
/// check `l10n::options_txt::update_atomically` performs on every write.
/// `options.txt` and the pack files under `.minecraft/` are files this
/// launcher shares with the game, so a write while the instance runs would
/// be discarded at best (the game rewrites `options.txt` wholesale on exit)
/// and race the game's own write at worst — see `l10n::options_txt`'s module
/// doc. `launch::registry()` is private with no cross-module seam, so the
/// guard itself can only be exercised end-to-end; kept as its own function
/// (rather than inlined into `l10n_apply`) purely so the DECISION is
/// unit-testable without a running instance.
fn apply_write_allowed(is_running: bool) -> Result<(), crate::error::Error> {
    if is_running {
        return Err(crate::error::Error::InstanceBusy);
    }
    Ok(())
}

/// Remove every Lucerna-generated resource pack — file and Add-ons registry
/// row — except `keep_filename`, best-effort.
///
/// Runs on EVERY `l10n_apply` call, not only when the target language
/// changes: it also sweeps up packs left behind by an older build of this
/// feature, or by a language the user has since abandoned. Only one Lucerna
/// pack is ever meant to exist — `options_txt::with_pack_enabled` already
/// enforces that for the `options.txt` ENTRY (it strips every prior
/// `PACK_PREFIX`-matching entry before appending the new one) — so leaving
/// the FILE and its Add-ons row behind after a language switch would be
/// inert but user-visible clutter: a resource pack the user never installed,
/// sitting in the Add-ons list with no explanation.
///
/// A removal failure here is untidiness, not a broken apply: logged via
/// `crate::diag!` and skipped, the same posture `ScanCache::update` takes on
/// a save failure. `keep_filename = None` sweeps every Lucerna pack — used on
/// the "nothing to ship" path, where no pack should survive at all.
async fn sweep_stale_lucerna_packs(inst_root: &std::path::Path, keep_filename: Option<&str>) {
    let installed = match crate::mods::assets::list(
        inst_root,
        crate::mods::platform::ContentKind::ResourcePack,
    )
    .await
    {
        Ok(items) => items,
        Err(e) => {
            crate::diag!("[l10n] apply: could not list resource packs to sweep: {e}");
            return;
        }
    };

    for asset in installed {
        if !asset
            .filename
            .starts_with(crate::l10n::options_txt::PACK_PREFIX)
        {
            continue; // not ours — never touched by this sweep
        }
        if keep_filename == Some(asset.filename.as_str()) {
            continue;
        }

        match crate::mods::install::safe_asset_remove_path(
            inst_root,
            crate::mods::platform::ContentKind::ResourcePack,
            &asset.filename,
        ) {
            Ok(path) => {
                if let Err(e) = tokio::fs::remove_file(&path).await {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        crate::diag!(
                            "[l10n] apply: could not remove stale pack {}: {e}",
                            asset.filename
                        );
                    }
                }
            }
            Err(e) => {
                crate::diag!(
                    "[l10n] apply: refusing to remove stale pack {} ({e})",
                    asset.filename
                );
            }
        }

        if let Err(e) = crate::mods::assets::remove(
            inst_root,
            crate::mods::platform::ContentKind::ResourcePack,
            &asset.filename,
        )
        .await
        {
            crate::diag!(
                "[l10n] apply: could not remove stale pack's registry row {}: {e}",
                asset.filename
            );
        }
    }
}

/// Build the override pack for `lang` from every namespace with overrides,
/// place it, register it in the Add-ons list, and enable it in
/// `options.txt`.
///
/// Returns whether `options.txt` activation happened. `false` covers two
/// distinct, non-error outcomes the UI must tell apart from a hard failure:
///   - nothing to ship — no overrides exist for `lang`, so any previously
///     generated pack (and its `options.txt` entry) is removed instead;
///   - the pack is on disk and registered, but the instance's
///     `options.txt` does not exist yet (never launched) — deferred, not
///     failed; the UI explains this rather than surfacing an error.
#[tauri::command]
#[specta::specta]
pub async fn l10n_apply(
    app: tauri::AppHandle,
    instance_id: String,
    lang: String,
) -> Result<bool, crate::error::Error> {
    crate::data_root::reject_if_fallen_back(&app)?;
    apply_write_allowed(crate::launch::spawn::is_running(&instance_id))?;

    let inst_root = instance_root(&app, &instance_id)?;
    let store_dir =
        crate::paths::l10n_dir(&app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let versions_dir = crate::paths::versions_dir(&app)
        .map_err(|e| crate::error::Error::io("<versions_dir>", e))?;
    let (mc_version, _loader) = read_active_mc_and_loader(&app, &instance_id)?;

    let client_jar = crate::datapacks::compat::client_jar_path(&versions_dir, &mc_version);
    let fmt = crate::l10n::pack_format::from_client_jar_path(&client_jar);
    match crate::l10n::pack_format::apply_gate(fmt) {
        crate::l10n::pack_format::ApplyGate::UnknownFormat => {
            return Err(crate::error::Error::L10nFormatUnknown { mc_version });
        }
        crate::l10n::pack_format::ApplyGate::TooOld => {
            return Err(crate::error::Error::L10nFormatTooOld { mc_version });
        }
        crate::l10n::pack_format::ApplyGate::Ready => {}
    }

    let namespaces = crate::l10n::store::namespaces_with_overrides(&store_dir, &lang);
    let stores: Vec<_> = namespaces
        .iter()
        .map(|ns| crate::l10n::store::load(&store_dir, &lang, ns))
        .collect();
    let description = format!("Lucerna translations ({lang})");
    let built = crate::l10n::pack::build(&stores, &lang, fmt, &description);

    let mc_dir = inst_root.join(".minecraft");
    let filename = format!("{}{lang}.zip", crate::l10n::options_txt::PACK_PREFIX);

    let Some(bytes) = built else {
        // Nothing to ship: sweep away every Lucerna pack, including this
        // language's own if one exists — `keep_filename: None` — plus the
        // options.txt entry. Emptying every override must leave no Lucerna
        // pack behind at all.
        sweep_stale_lucerna_packs(&inst_root, None).await;
        crate::l10n::options_txt::update_atomically(
            &mc_dir,
            crate::launch::spawn::is_running(&instance_id),
            |s| Some(crate::l10n::options_txt::with_pack_disabled(s)),
        )?;
        return Ok(false);
    };

    // Sweep BEFORE writing: by the time `install_asset_local` runs, no OTHER
    // Lucerna pack can be sitting in the Add-ons list. The sweep excludes
    // `filename` itself, so a re-apply of the SAME language never touches
    // the file/row this call is about to write — there is no ordering race
    // to get wrong here, only tidiness to get right before the new pack
    // lands rather than after.
    sweep_stale_lucerna_packs(&inst_root, Some(&filename)).await;

    // The sanctioned sink (temp-sibling + rename, so a pack that already
    // exists at this filename is never written through) plus the manual-
    // install registry convention (`source: None`) — see
    // `mods::asset_local::install_asset_local`'s own doc comment.
    crate::mods::asset_local::install_asset_local(
        &inst_root,
        crate::mods::platform::ContentKind::ResourcePack,
        &filename,
        &bytes,
    )
    .await?;

    // `ApplyGate::Ready` already proved `supports_apply(fmt)`, i.e. resource
    // format >= 4 — Minecraft 1.13+, which is exactly when `options.txt`
    // stores user pack ids under the `file/` prefix (see `l10n::options_txt`'s
    // module doc). Re-snapshot `is_running` rather than reusing the guard
    // above: real time has passed doing the work above this line.
    let activated = crate::l10n::options_txt::update_atomically(
        &mc_dir,
        crate::launch::spawn::is_running(&instance_id),
        |s| {
            Some(crate::l10n::options_txt::with_pack_enabled(
                s, &filename, true,
            ))
        },
    )?;

    Ok(activated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_language_setting_falls_back_to_english() {
        assert_eq!(resolve_ui_language("system"), "en_us");
    }

    #[test]
    fn a_real_language_setting_is_mapped_through_default_target_code() {
        assert_eq!(resolve_ui_language("ru"), "ru_ru");
        assert_eq!(resolve_ui_language("pt-BR"), "pt_br");
    }

    // -----------------------------------------------------------------
    // validate_override_identifiers
    // -----------------------------------------------------------------

    #[test]
    fn validate_override_identifiers_rejects_a_slash_bearing_namespace() {
        let err = validate_override_identifiers("../../evil", "ru_ru").unwrap_err();
        assert!(matches!(
            err,
            crate::error::Error::L10nNamespaceInvalid { namespace } if namespace == "../../evil"
        ));
    }

    #[test]
    fn validate_override_identifiers_rejects_a_slash_bearing_lang() {
        let err = validate_override_identifiers("create", "../../evil").unwrap_err();
        assert!(matches!(
            err,
            crate::error::Error::L10nLangInvalid { lang } if lang == "../../evil"
        ));
    }

    #[test]
    fn validate_override_identifiers_checks_namespace_before_lang() {
        // Both are bad: the namespace error must win, so a caller always
        // learns about the FIRST problem rather than a nondeterministic one.
        let err = validate_override_identifiers("..", "..").unwrap_err();
        assert!(matches!(
            err,
            crate::error::Error::L10nNamespaceInvalid { .. }
        ));
    }

    #[test]
    fn validate_override_identifiers_accepts_ordinary_values() {
        assert!(validate_override_identifiers("create", "ru_ru").is_ok());
    }

    // -----------------------------------------------------------------
    // apply_write_allowed
    // -----------------------------------------------------------------

    #[test]
    fn apply_write_allowed_permits_a_stopped_instance() {
        assert!(apply_write_allowed(false).is_ok());
    }

    #[test]
    fn apply_write_allowed_refuses_a_running_instance() {
        assert!(matches!(
            apply_write_allowed(true).unwrap_err(),
            crate::error::Error::InstanceBusy
        ));
    }

    // -----------------------------------------------------------------
    // sweep_stale_lucerna_packs (+ the sweep-then-write sequence l10n_apply
    // performs, exercised directly since it needs no AppHandle)
    // -----------------------------------------------------------------

    /// A real generated override pack for `lang`, via `l10n::pack::build` —
    /// exercises the exact bytes `l10n_apply` would produce, not a hand-rolled
    /// stand-in.
    fn generated_pack(lang: &str) -> Vec<u8> {
        let mut s = crate::l10n::store::NamespaceStore::new("create", lang);
        s.set("item.create.wrench", "x", "Wrench", 1.0);
        crate::l10n::pack::build(
            &[s],
            lang,
            crate::l10n::pack_format::PackFormat {
                major: 34,
                minor: 0,
            },
            "Lucerna translations",
        )
        .expect("has an override, known format")
    }

    /// A minimal but valid hand-authored resource pack — `pack.mcmeta` plus
    /// an `assets/` tree, no `data/` tree — so `install_asset_local`'s
    /// `pack_meta::classify` accepts it. Stands in for a pack the USER
    /// installed, which the sweep must never touch.
    fn minimal_resource_pack_zip() -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            w.start_file("pack.mcmeta", opts).unwrap();
            w.write_all(br#"{"pack":{"pack_format":15,"description":"x"}}"#)
                .unwrap();
            w.start_file("assets/minecraft/textures/x.png", opts)
                .unwrap();
            w.write_all(b"\x89PNG").unwrap();
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    fn resourcepacks_dir(inst_root: &std::path::Path) -> std::path::PathBuf {
        inst_root.join(".minecraft").join("resourcepacks")
    }

    async fn installed_resource_packs(
        inst_root: &std::path::Path,
    ) -> Vec<crate::mods::platform::InstalledAsset> {
        crate::mods::assets::list(inst_root, crate::mods::platform::ContentKind::ResourcePack)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn two_applies_with_different_languages_leave_exactly_one_pack_the_second() {
        let td = tempfile::tempdir().unwrap();
        let inst_root = td.path();

        let ru = format!("{}ru_ru.zip", crate::l10n::options_txt::PACK_PREFIX);
        sweep_stale_lucerna_packs(inst_root, Some(&ru)).await;
        crate::mods::asset_local::install_asset_local(
            inst_root,
            crate::mods::platform::ContentKind::ResourcePack,
            &ru,
            &generated_pack("ru_ru"),
        )
        .await
        .unwrap();

        let de = format!("{}de_de.zip", crate::l10n::options_txt::PACK_PREFIX);
        sweep_stale_lucerna_packs(inst_root, Some(&de)).await;
        crate::mods::asset_local::install_asset_local(
            inst_root,
            crate::mods::platform::ContentKind::ResourcePack,
            &de,
            &generated_pack("de_de"),
        )
        .await
        .unwrap();

        let installed = installed_resource_packs(inst_root).await;
        assert_eq!(
            installed.len(),
            1,
            "must leave exactly one pack: {installed:?}"
        );
        assert_eq!(
            installed[0].filename, de,
            "the survivor must be the SECOND apply"
        );

        // Not just the registry row — the ru_ru FILE must be gone too.
        assert!(!resourcepacks_dir(inst_root).join(&ru).exists());
        assert!(resourcepacks_dir(inst_root).join(&de).exists());
    }

    #[tokio::test]
    async fn reapplying_the_same_language_twice_still_leaves_exactly_one_pack() {
        // Confirms the sweep-then-write ordering doesn't race itself on a
        // same-filename re-apply: `mods::assets::add` already dedups by
        // `(kind, filename)` and `place_bytes` already overwrites atomically,
        // and the sweep explicitly excludes `keep_filename`, so the sweep
        // never touches the very file this call is about to (re)write.
        let td = tempfile::tempdir().unwrap();
        let inst_root = td.path();
        let filename = format!("{}ru_ru.zip", crate::l10n::options_txt::PACK_PREFIX);

        for _ in 0..2 {
            sweep_stale_lucerna_packs(inst_root, Some(&filename)).await;
            crate::mods::asset_local::install_asset_local(
                inst_root,
                crate::mods::platform::ContentKind::ResourcePack,
                &filename,
                &generated_pack("ru_ru"),
            )
            .await
            .unwrap();
        }

        let installed = installed_resource_packs(inst_root).await;
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].filename, filename);
        assert!(resourcepacks_dir(inst_root).join(&filename).exists());
    }

    #[tokio::test]
    async fn sweep_with_no_keep_filename_removes_every_lucerna_pack() {
        // Mirrors the "nothing to ship" path in `l10n_apply`: emptying every
        // override must leave no Lucerna pack behind at all.
        let td = tempfile::tempdir().unwrap();
        let inst_root = td.path();
        let filename = format!("{}ru_ru.zip", crate::l10n::options_txt::PACK_PREFIX);
        crate::mods::asset_local::install_asset_local(
            inst_root,
            crate::mods::platform::ContentKind::ResourcePack,
            &filename,
            &generated_pack("ru_ru"),
        )
        .await
        .unwrap();

        sweep_stale_lucerna_packs(inst_root, None).await;

        assert!(installed_resource_packs(inst_root).await.is_empty());
        assert!(!resourcepacks_dir(inst_root).join(&filename).exists());
    }

    #[tokio::test]
    async fn sweep_never_touches_a_pack_the_user_installed_by_hand() {
        let td = tempfile::tempdir().unwrap();
        let inst_root = td.path();
        crate::mods::asset_local::install_asset_local(
            inst_root,
            crate::mods::platform::ContentKind::ResourcePack,
            "Faithful.zip",
            &minimal_resource_pack_zip(),
        )
        .await
        .unwrap();

        sweep_stale_lucerna_packs(inst_root, None).await;

        let installed = installed_resource_packs(inst_root).await;
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].filename, "Faithful.zip");
        assert!(resourcepacks_dir(inst_root).join("Faithful.zip").exists());
    }

    #[tokio::test]
    async fn sweep_on_an_instance_with_no_resource_packs_at_all_is_a_harmless_no_op() {
        let td = tempfile::tempdir().unwrap();
        sweep_stale_lucerna_packs(td.path(), None).await;
        assert!(installed_resource_packs(td.path()).await.is_empty());
    }
}

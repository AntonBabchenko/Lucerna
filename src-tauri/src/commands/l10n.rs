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
    crate::l10n::apply::rebuild_pack(&app, &instance_id, &lang).await
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
}

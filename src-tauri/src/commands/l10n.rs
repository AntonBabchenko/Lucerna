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

/// One hit: the row the editor already knows how to render, plus the mod it
/// came from — which is the whole point, because the user searching does not
/// know that.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub namespace: String,
    pub row: KeyRow,
}

/// What an instance-wide search found, and what it could not see.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Best matches first.
    pub hits: Vec<SearchHit>,
    /// Mods present but switched off. Their strings were NOT searched, and an
    /// empty result means something different when this is non-zero — the user
    /// can act on that, so it must not be silently omitted.
    pub disabled_mods: u32,
    /// True when `hits` was cut at [`SEARCH_HIT_CAP`]. Reported rather than
    /// silently truncated: a capped list that claims to be complete is worse
    /// than one that admits it stopped counting.
    pub truncated: bool,
}

/// Enough hits to find anything real; short enough that a one-letter query
/// cannot drag a hundred-mod pack's entire key space across IPC.
const SEARCH_HIT_CAP: usize = 200;

/// Find a string anywhere in this instance, without knowing its mod.
///
/// The entry point for the case the per-namespace editor cannot serve: the
/// player saw a line in game and has the TEXT, not the mod. Matching is
/// forgiving on purpose — see `l10n::find` for why a literal substring search
/// finds neither `Invalid language: %s` nor `§dServux: %s§r`.
///
/// Searches the key, the mod's English, AND the user's own override. The
/// per-mod search deliberately excluded the override on the grounds that the
/// target script may be unreadable to the user; that holds for someone
/// translating INTO an alphabet they do not know, and not at all for someone
/// looking for a phrasing they themselves wrote and now dislike.
#[tauri::command]
#[specta::specta]
pub async fn l10n_search(
    app: tauri::AppHandle,
    instance_id: String,
    lang: String,
    query: String,
) -> Result<SearchResult, crate::error::Error> {
    let inst_root = instance_root(&app, &instance_id)?;
    let store_dir =
        crate::paths::l10n_dir(&app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let lang = crate::l10n::coverage::default_target_code(&lang);

    let disabled_mods = crate::mods::installed::list(&inst_root)
        .await
        .map(|mods| mods.iter().filter(|m| !m.enabled).count() as u32)
        .unwrap_or(0);

    if crate::l10n::find::normalise(&query).is_empty() {
        return Ok(SearchResult {
            hits: Vec::new(),
            disabled_mods,
            truncated: false,
        });
    }

    let maps = crate::l10n::namespace_scan::instance_lang_maps(&inst_root, &lang).await?;

    let mut ranked: Vec<(crate::l10n::find::Rank, SearchHit)> = Vec::new();
    for (namespace, (en, mod_tr)) in &maps {
        let store = crate::l10n::store::load(&store_dir, &lang, namespace);
        for row in crate::l10n::store::namespace_key_rows(&store, en, Some(mod_tr)) {
            let best = [
                Some(row.key.as_str()),
                Some(row.source_en.as_str()),
                row.override_value.as_deref(),
            ]
            .into_iter()
            .flatten()
            .filter_map(|value| crate::l10n::find::rank(&query, value))
            .max();
            if let Some(rank) = best {
                ranked.push((
                    rank,
                    SearchHit {
                        namespace: namespace.clone(),
                        row,
                    },
                ));
            }
        }
    }

    // Best first, then a stable, predictable order so the same query twice
    // never reshuffles rows under the user's cursor.
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.namespace.cmp(&b.1.namespace))
            .then_with(|| a.1.row.key.cmp(&b.1.row.key))
    });

    let truncated = ranked.len() > SEARCH_HIT_CAP;
    ranked.truncate(SEARCH_HIT_CAP);

    Ok(SearchResult {
        hits: ranked.into_iter().map(|(_, hit)| hit).collect(),
        disabled_mods,
        truncated,
    })
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

// =========================================================================
// In-game mod localization — AI pre-fill
// =========================================================================

pub use crate::l10n::prefill::estimate::PrefillEstimate;
pub use crate::l10n::prefill::run::{PrefillProgress, RunSummary};

/// The same boundary check [`validate_override_identifiers`] applies, for the
/// pre-fill commands — whose `namespace` is an optional SCOPE rather than a
/// value destined for the store.
///
/// `lang` needs the check for exactly the reason `l10n_set_override` does: a
/// run writes through `l10n::store::load`, which stamps the raw `lang` into
/// `NamespaceStore::lang` and then persists it INSIDE the JSON body, where
/// `store_path`'s percent-encoding of the file NAME never reaches it.
///
/// `None` means "the whole instance", so there is no namespace to check. The
/// empty string stands in for that case rather than a placeholder name: it is
/// the one value guaranteed to pass `is_traversal_unsafe` (not `.`, not `..`,
/// no separator, no control character), and inventing a fake namespace to
/// validate would be a lie about what was checked. Pinned by
/// `validate_prefill_scope_accepts_a_whole_instance_run` below.
fn validate_prefill_scope(namespace: Option<&str>, lang: &str) -> Result<(), crate::error::Error> {
    validate_override_identifiers(namespace.unwrap_or(""), lang)
}

/// What a pre-fill run would cost, before committing to it: how many keys are
/// missing, how many of those a previous run's cache or vanilla Minecraft
/// already answers, and how many distinct strings would actually reach the
/// model.
///
/// `namespace` scopes the estimate to one resource namespace; `None` covers
/// the whole instance. Resolved through the same discovery, cache and glossary
/// the run itself uses — see `l10n::prefill::run::estimate`. That also means it
/// inherits the run's preflight, so a missing API key or an unset local model
/// name surfaces here rather than after the user has confirmed a number.
#[tauri::command]
#[specta::specta]
pub async fn l10n_prefill_estimate(
    app: tauri::AppHandle,
    instance_id: String,
    lang: String,
    namespace: Option<String>,
) -> Result<PrefillEstimate, crate::error::Error> {
    validate_prefill_scope(namespace.as_deref(), &lang)?;
    crate::l10n::prefill::run::estimate(&app, &instance_id, &lang, namespace.as_deref()).await
}

/// Fill the instance's missing translations for `lang` with the configured
/// model, then rebuild the override pack.
///
/// Progress ticks arrive on `on_progress` — a plain `Channel`, adapted to the
/// domain module's closure here so `tauri::ipc` never reaches
/// `l10n::prefill`. The run is cancellable through [`l10n_prefill_cancel`],
/// and a second run for the same instance is refused while one is registered:
/// two at once would race each other's pack rebuild, and the second would
/// silently pay for strings the first was already buying.
///
/// The refusal is a check followed by a registration, not one atomic step, so
/// two invocations landing on different worker threads within the same
/// instant can both pass it — the same shape `server_cancel_upload`'s registry
/// has. The damage is bounded rather than absent: each flush re-reads the
/// namespace file before saving, so neither run can delete the other's
/// entries; what the loser costs is duplicate spend and a pack rebuilt twice.
/// Closing the window means a check-and-insert under one lock in
/// `prefill::cancel`, which is worth doing the next time that module is open.
///
/// A failure part-way through is reported ON the returned summary
/// (`RunSummary::failed`), not in place of it — everything written before the
/// failure was verified and paid for, and the pack is rebuilt around it.
#[tauri::command]
#[specta::specta]
pub async fn l10n_prefill_start(
    app: tauri::AppHandle,
    instance_id: String,
    lang: String,
    namespace: Option<String>,
    on_progress: Channel<PrefillProgress>,
) -> Result<RunSummary, crate::error::Error> {
    crate::data_root::reject_if_fallen_back(&app)?;
    // The run finishes by rebuilding the pack inside the instance and touching
    // `options.txt` — the same files `l10n_apply` refuses to write while the
    // game is running, for the same reason.
    apply_write_allowed(crate::launch::spawn::is_running(&instance_id))?;
    if crate::l10n::prefill::cancel::is_active(&instance_id) {
        return Err(crate::error::Error::L10nPrefillBusy);
    }
    validate_prefill_scope(namespace.as_deref(), &lang)?;

    let cancel = crate::l10n::prefill::cancel::begin(&instance_id);
    let outcome = crate::l10n::prefill::run::run(
        &app,
        &instance_id,
        &lang,
        namespace.as_deref(),
        &cancel,
        &|tick| {
            // A closed channel (the dialog was dismissed) is not a reason to
            // stop translating — the run's own cancel flag is.
            let _ = on_progress.send(tick);
        },
    )
    .await;
    // Not `?` on the line above, and not a guard object either: the flag has to
    // be de-registered on EVERY path, or one failed run leaves the instance
    // permanently "busy" until the launcher restarts.
    crate::l10n::prefill::cancel::end(&instance_id);
    outcome
}

/// Ask the in-flight pre-fill run for `instance_id` to stop. A no-op when
/// none is running. Sync and trivial, like `server_cancel_upload`: it flips a
/// flag the run polls between batches, and the run itself returns the summary
/// of what it had already bought.
#[tauri::command]
#[specta::specta]
pub fn l10n_prefill_cancel(instance_id: String) -> Result<(), crate::error::Error> {
    crate::l10n::prefill::cancel::cancel(&instance_id);
    Ok(())
}

/// Store (or clear) the API key for one AI provider in the OS keyring.
///
/// An empty `key` CLEARS the stored credential, matching `l10n_set_override`'s
/// convention for the same shape of call. The key is trimmed before it is
/// stored, exactly as it is trimmed after it is read, so a pasted trailing
/// newline can never make a stored key look present and then be sent verbatim.
///
/// `provider` is the typed [`AiProvider`](crate::instances::schema::AiProvider)
/// rather than its string id: the keyring sweep that runs on uninstall deletes
/// only the ids `AiProvider::ALL` names, so a key filed under an unrecognised
/// string would outlive the uninstall it was meant to be removed by. Taking
/// the enum makes that unrepresentable instead of validated.
#[tauri::command]
#[specta::specta]
pub async fn l10n_prefill_set_key(
    provider: crate::instances::schema::AiProvider,
    key: String,
) -> Result<(), crate::error::Error> {
    let slot = crate::accounts::keychain::ai_provider_key(provider.id());
    let key = key.trim();
    if key.is_empty() {
        return crate::accounts::keychain::delete(&slot);
    }
    crate::accounts::keychain::store(&slot, key)
}

/// Whether an API key is stored for `provider`.
///
/// A bool, never the secret: there is deliberately no command anywhere that
/// reads a stored key back out to the UI, so the Settings field starts empty
/// on every open and a stored key can be replaced but never displayed.
#[tauri::command]
#[specta::specta]
pub async fn l10n_prefill_key_status(
    provider: crate::instances::schema::AiProvider,
) -> Result<bool, crate::error::Error> {
    crate::l10n::prefill::run::has_stored_key(provider)
}

/// Round-trip the configured provider, model and credential with the smallest
/// possible request. Returns `Ok(())` when the provider accepted it; the
/// answer itself is discarded, since what is being tested is auth, endpoint
/// and model name, and every one of those failures arrives as a status code.
///
/// This sends the user's key to a third party, so it takes the same consent
/// token a run does — minted here, because the button is not downstream of
/// `prefill::run` at all.
#[tauri::command]
#[specta::specta]
pub async fn l10n_prefill_test_key(app: tauri::AppHandle) -> Result<(), crate::error::Error> {
    let consent = crate::network::consent::ai_consent(&app)?;
    let cfg = crate::l10n::prefill::run::resolve_provider(&app)?;
    crate::l10n::prefill::provider::test_credentials(
        &consent,
        cfg.provider,
        cfg.api_key.as_deref(),
        cfg.local_port,
        &cfg.model,
    )
    .await
}

/// Drop every machine-written override in one namespace and rebuild the pack.
/// Returns how many entries were removed.
///
/// One load and one save of the single file that holds them, not one per key:
/// the alternative — N× `l10n_set_override` — would be N IPC round-trips and N
/// full load-plus-atomic-save cycles of the same file, which for a namespace
/// this feature has just filled means two thousand of each.
///
/// A hand-edited machine string is `Origin::Manual` (the editor's save path
/// reclaims it), so this only ever removes what the user never touched.
///
/// The pack is rebuilt even when nothing was removed. The store and the pack
/// have to agree at the end of this command, and "nothing to remove" is
/// exactly the state a previously failed rebuild leaves behind — skipping it
/// there would make that state unrecoverable by retrying.
#[tauri::command]
#[specta::specta]
pub async fn l10n_revert_machine(
    app: tauri::AppHandle,
    instance_id: String,
    lang: String,
    namespace: String,
) -> Result<u32, crate::error::Error> {
    crate::data_root::reject_if_fallen_back(&app)?;
    apply_write_allowed(crate::launch::spawn::is_running(&instance_id))?;
    validate_override_identifiers(&namespace, &lang)?;

    let store_dir =
        crate::paths::l10n_dir(&app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let mut store = crate::l10n::store::load(&store_dir, &lang, &namespace);
    let before = store.entries.len();
    store
        .entries
        .retain(|_key, entry| entry.origin != crate::l10n::store::Origin::Machine);
    let removed = before - store.entries.len();

    if removed > 0 {
        crate::l10n::store::save(&store_dir, &store)
            .map_err(|e| crate::error::Error::io("<l10n override store>", e))?;
    }
    // Propagated, not swallowed: unlike a pre-fill run — which has an hour of
    // paid work to protect — a revert that cannot reach the pack has left the
    // game still showing every string the user asked to be rid of, and saying
    // so is the only honest answer.
    crate::l10n::apply::rebuild_pack(&app, &instance_id, &lang).await?;
    Ok(u32::try_from(removed).unwrap_or(u32::MAX))
}

// =========================================================================
// In-game mod localization — share bundles + cross-instance apply targets
// =========================================================================

pub use crate::l10n::share::{BundleSummary, ConflictPolicy, ImportResult};
pub use crate::l10n::targets::ApplyTarget;

/// Every instance's translation-pack state for `lang` — the single data source
/// behind the "apply in other instances" offer and the Overview badge.
///
/// Runs the same scan-and-cache path `l10n_coverage` uses, per instance. The
/// jar-scan cache is keyed by (language, jar SHA-1) and filled lazily, so an
/// instance whose translations were never opened for this language is a cache
/// MISS and its jars are read once here — which is exactly the flagship case
/// (the same mod at a different version in another instance). Callers treat
/// this as async work with a spinner; repeat calls are cache hits.
#[tauri::command]
#[specta::specta]
pub async fn l10n_apply_targets(
    app: tauri::AppHandle,
    lang: String,
) -> Result<Vec<ApplyTarget>, crate::error::Error> {
    validate_override_identifiers("", &lang)?;
    let store_dir =
        crate::paths::l10n_dir(&app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let cache_path = crate::paths::l10n_scan_cache_file(&app)
        .map_err(|e| crate::error::Error::io("<l10n_scan_cache_file>", e))?;
    let versions_dir = crate::paths::versions_dir(&app)
        .map_err(|e| crate::error::Error::io("<versions_dir>", e))?;
    let lang = crate::l10n::coverage::default_target_code(&lang);

    let overridden: std::collections::BTreeSet<String> =
        crate::l10n::store::namespaces_with_overrides(&store_dir, &lang)
            .into_iter()
            .collect();

    let mut out = Vec::new();
    for inst in crate::instances::list_instances_with_status(&app)? {
        let Ok(inst_root) = instance_root(&app, &inst.id) else {
            continue; // an instance that vanished between the listing and here
        };
        let mc_version = read_active_mc_and_loader(&app, &inst.id)
            .map(|(mc, _loader)| mc)
            .unwrap_or_default();
        // A single unscannable instance — a broken mods dir, a jar that
        // vanished mid-scan — must not empty the whole offer. Skip it: this
        // list's job is to name the instances we CAN help with.
        let Ok(cov) = crate::l10n::coverage::scan_instance(
            &inst_root,
            &cache_path,
            &store_dir,
            &versions_dir,
            &mc_version,
            &lang,
        )
        .await
        else {
            continue;
        };

        let covered = cov
            .namespaces
            .iter()
            .any(|n| overridden.contains(&n.namespace));
        let client_jar = crate::datapacks::compat::client_jar_path(&versions_dir, &mc_version);
        let fmt = crate::l10n::pack_format::from_client_jar_path(&client_jar);
        // Rebuild with THIS instance's format. The pack embeds `pack.mcmeta`,
        // whose contents depend on the Minecraft version, so one rebuild shared
        // across a mixed-version fleet would mark every instance but one
        // permanently outdated.
        let rebuild = match cov.apply_gate {
            crate::l10n::pack_format::ApplyGate::Ready => {
                crate::l10n::apply::assemble_current_pack(&store_dir, &lang, fmt)
            }
            _ => None,
        };
        let pack_path = inst_root
            .join(".minecraft")
            .join("resourcepacks")
            .join(format!(
                "{}{lang}.zip",
                crate::l10n::options_txt::PACK_PREFIX
            ));
        let disk = std::fs::read(&pack_path).ok();
        let enabled = matches!(cov.pack_state, crate::l10n::options_txt::PackState::Enabled);
        let state = crate::l10n::targets::classify(
            disk.as_deref(),
            rebuild.as_deref(),
            enabled,
            cov.apply_gate,
        );
        let empty_rebuild_with_pack = disk.is_some()
            && rebuild.is_none()
            && matches!(cov.apply_gate, crate::l10n::pack_format::ApplyGate::Ready);
        let candidate = crate::l10n::targets::candidacy(covered, state, empty_rebuild_with_pack);
        let is_running = crate::launch::spawn::is_running(&inst.id);
        let prefill_active = crate::l10n::prefill::cancel::is_active(&inst.id);
        out.push(ApplyTarget {
            instance_id: inst.id.clone(),
            name: inst.name.clone(),
            covered,
            state,
            applied_other_lang: other_lang_applied(&inst_root, &lang),
            is_running,
            prefill_active,
            candidate,
            actionable: crate::l10n::targets::actionable(candidate, is_running, prefill_active),
        });
    }
    Ok(out)
}

/// The language code of a DIFFERENT Lucerna pack currently enabled on this
/// instance, if any.
///
/// Exactly one Lucerna pack exists per instance and every apply sweeps the
/// others away, so applying `lang` here would REPLACE whatever language is
/// applied now. A bulk action must disclose that rather than silently
/// overwriting a deliberate per-instance choice.
///
/// Best-effort throughout: an unreadable `options.txt` or resourcepacks
/// directory means "nothing to disclose", the same tolerance the sweep itself
/// takes.
fn other_lang_applied(inst_root: &std::path::Path, lang: &str) -> Option<String> {
    let mc_dir = inst_root.join(".minecraft");
    let options = std::fs::read_to_string(mc_dir.join("options.txt")).ok()?;
    let dir = std::fs::read_dir(mc_dir.join("resourcepacks")).ok()?;
    for entry in dir.flatten() {
        // `continue`, never `?`: one filename that is not valid UTF-8 must skip
        // that entry, not abandon the scan and report "no other language" when
        // there is one.
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(rest) = name.strip_prefix(crate::l10n::options_txt::PACK_PREFIX) else {
            continue;
        };
        let Some(other) = rest.strip_suffix(".zip") else {
            continue;
        };
        if other != lang && options.contains(&name) {
            return Some(other.to_string());
        }
    }
    None
}

/// Namespaces holding at least one override for `lang`.
///
/// Feeds the export dialog's checkbox list. Deliberately NOT scoped to an
/// instance: the override store is global, so a user must be able to export a
/// mod that is not installed in whichever instance they happen to have open.
/// The caller uses its own instance's mods only to decide which boxes start
/// ticked.
#[tauri::command]
#[specta::specta]
pub fn l10n_overridden_namespaces(
    app: tauri::AppHandle,
    lang: String,
) -> Result<Vec<String>, crate::error::Error> {
    validate_override_identifiers("", &lang)?;
    let store_dir =
        crate::paths::l10n_dir(&app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let lang = crate::l10n::coverage::default_target_code(&lang);
    Ok(crate::l10n::store::namespaces_with_overrides(
        &store_dir, &lang,
    ))
}

/// Write a share bundle to `dest_path`.
///
/// The save dialog lives in the FRONTEND (the established pattern — see the
/// modpack export command, which likewise takes a destination the UI chose);
/// taking a path rather than opening a dialog here is also what keeps this
/// write testable against a temp directory.
///
/// Keyed on `mc_version` rather than an instance id so the eventual global
/// translations manager, which has no instance context, can call it unchanged
/// with a version the user picks.
#[tauri::command]
#[specta::specta]
pub async fn l10n_export(
    app: tauri::AppHandle,
    mc_version: String,
    lang: String,
    namespaces: Vec<String>,
    note: String,
    dest_path: String,
) -> Result<(), crate::error::Error> {
    crate::data_root::reject_if_fallen_back(&app)?;
    validate_override_identifiers("", &lang)?;
    if namespaces.is_empty() {
        return Err(crate::error::Error::L10nShareNothingToExport);
    }
    let dest = std::path::PathBuf::from(&dest_path);
    if dest
        .file_name()
        .and_then(|n| n.to_str())
        .is_none_or(crate::l10n::share::dest_filename_reserved)
    {
        return Err(crate::error::Error::L10nShareDestReserved);
    }
    let versions_dir = crate::paths::versions_dir(&app)
        .map_err(|e| crate::error::Error::io("<versions_dir>", e))?;
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
    let store_dir =
        crate::paths::l10n_dir(&app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let lang = crate::l10n::coverage::default_target_code(&lang);
    let stores: Vec<_> = namespaces
        .iter()
        .filter(|ns| !crate::l10n::scan::is_traversal_unsafe(ns))
        .map(|ns| crate::l10n::store::load(&store_dir, &lang, ns))
        .collect();
    let bytes = crate::l10n::share::build_bundle(&stores, &lang, fmt, &note)
        .ok_or(crate::error::Error::L10nShareNothingToExport)?;
    tokio::fs::write(&dest, bytes)
        .await
        .map_err(|e| crate::error::Error::io("<share bundle write>", e))
}

/// Parse and validate a bundle WITHOUT writing anything — feeds the import
/// summary dialog. Advisory only: [`l10n_import_bundle`] re-reads and
/// re-validates from scratch, because the file can change between the two
/// calls.
#[tauri::command]
#[specta::specta]
pub async fn l10n_inspect_bundle(
    app: tauri::AppHandle,
    path: String,
) -> Result<BundleSummary, crate::error::Error> {
    let store_dir =
        crate::paths::l10n_dir(&app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| crate::error::Error::io("<share bundle read>", e))?;
    let parsed = crate::l10n::share::parse_bundle_bytes(&bytes)
        .map_err(|error| crate::error::Error::L10nShareBundleInvalid { error })?;
    Ok(crate::l10n::share::inspect(&store_dir, &parsed))
}

/// Merge a bundle into the global override store under `policy`.
///
/// Gated on the data root and on any active AI pre-fill run — that run writes
/// the same per-(language, namespace) files and `store::save` rewrites whole
/// files, so the two would be last-writer-wins. There is deliberately NO
/// running-game gate: an import touches only the store, never an instance.
#[tauri::command]
#[specta::specta]
pub async fn l10n_import_bundle(
    app: tauri::AppHandle,
    path: String,
    policy: ConflictPolicy,
) -> Result<ImportResult, crate::error::Error> {
    crate::data_root::reject_if_fallen_back(&app)?;
    if crate::l10n::prefill::cancel::any_active() {
        return Err(crate::error::Error::L10nSharePrefillActive);
    }
    let store_dir =
        crate::paths::l10n_dir(&app).map_err(|e| crate::error::Error::io("<l10n_dir>", e))?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| crate::error::Error::io("<share bundle read>", e))?;
    let parsed = crate::l10n::share::parse_bundle_bytes(&bytes)
        .map_err(|error| crate::error::Error::L10nShareBundleInvalid { error })?;
    Ok(crate::l10n::share::merge(
        &store_dir,
        &parsed,
        policy,
        crate::instances::unix_ms_f64(),
    ))
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
    // validate_prefill_scope
    // -----------------------------------------------------------------

    #[test]
    fn validate_prefill_scope_accepts_a_whole_instance_run() {
        // `None` is the whole-instance case, and it is spelled as the empty
        // string. If `is_traversal_unsafe` ever started rejecting `""`, every
        // unscoped pre-fill would fail with "invalid namespace" naming a
        // namespace the caller never supplied.
        assert!(validate_prefill_scope(None, "ru_ru").is_ok());
    }

    #[test]
    fn validate_prefill_scope_still_checks_the_language_without_a_namespace() {
        // The reason the check exists at all: a run stamps `lang` into every
        // namespace store it writes, and the store persists it inside the JSON
        // body where the file-name sanitiser never reaches it.
        assert!(matches!(
            validate_prefill_scope(None, "../../evil").unwrap_err(),
            crate::error::Error::L10nLangInvalid { .. }
        ));
    }

    #[test]
    fn validate_prefill_scope_checks_a_scoped_namespace() {
        assert!(matches!(
            validate_prefill_scope(Some("../../evil"), "ru_ru").unwrap_err(),
            crate::error::Error::L10nNamespaceInvalid { .. }
        ));
        assert!(validate_prefill_scope(Some("create"), "ru_ru").is_ok());
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
    // other_lang_applied
    // -----------------------------------------------------------------

    #[test]
    fn other_lang_applied_reports_only_a_different_enabled_language() {
        let td = tempfile::tempdir().unwrap();
        let mc = td.path().join(".minecraft");
        let packs = mc.join("resourcepacks");
        std::fs::create_dir_all(&packs).unwrap();
        let de = format!("{}de_de.zip", crate::l10n::options_txt::PACK_PREFIX);
        std::fs::write(packs.join(&de), b"zip").unwrap();
        std::fs::write(
            mc.join("options.txt"),
            format!("resourcePacks:[\"vanilla\",\"file/{de}\"]\n"),
        )
        .unwrap();

        // Asking about ru_ru finds the de_de pack that applying would replace.
        assert_eq!(
            other_lang_applied(td.path(), "ru_ru"),
            Some("de_de".to_string())
        );
        // Asking about the language that IS applied discloses nothing.
        assert_eq!(other_lang_applied(td.path(), "de_de"), None);
    }

    #[test]
    fn other_lang_applied_ignores_a_pack_file_that_options_txt_does_not_list() {
        // A pack left on disk but not enabled changes nothing in game, so it is
        // not something applying would "replace" from the user's point of view.
        let td = tempfile::tempdir().unwrap();
        let mc = td.path().join(".minecraft");
        let packs = mc.join("resourcepacks");
        std::fs::create_dir_all(&packs).unwrap();
        std::fs::write(
            packs.join(format!(
                "{}de_de.zip",
                crate::l10n::options_txt::PACK_PREFIX
            )),
            b"zip",
        )
        .unwrap();
        std::fs::write(mc.join("options.txt"), "resourcePacks:[\"vanilla\"]\n").unwrap();

        assert_eq!(other_lang_applied(td.path(), "ru_ru"), None);
    }
}

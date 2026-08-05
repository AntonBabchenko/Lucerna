//! The orchestrator: consent, plan, batches, verification, writes.
//!
//! The shape of a run, in order:
//!
//! 1. **Consent first, by construction.** [`run`]'s very first statement mints
//!    a `network::consent::AiConsent`, which is the only thing that reads the
//!    `AiTranslation` permission. `provider::complete` requires one, and the
//!    token's field is private to `network::consent`, so no code path here can
//!    reach a model without it — the gate does not depend on this file's
//!    statement order, and it covers `provider::test_credentials` (the
//!    Settings "Test key" button) too, which is not downstream of `run` at
//!    all. The network layer itself cannot help: the cloud hosts are on the
//!    allowlist and 127.0.0.1 bypasses it by design.
//! 2. **Discover.** One pass over the instance's jars
//!    (`namespace_scan::instance_lang_maps`) yields every namespace's English
//!    and mod-supplied translation at once; the store turns those into key
//!    rows, and a row that is `Missing` with a non-blank English string is one
//!    unit of work.
//! 3. **Spend nothing you do not have to.** Dedup (`plan::build_batches`),
//!    then the on-disk answer cache, then vanilla's own translations. Only
//!    what survives all three reaches a model.
//! 4. **Verify everything.** An answer is written only if `prefill::verify`
//!    accepts it. A rejected string gets exactly one retry with the verifier's
//!    complaint fed back; still rejected and it is simply not written — which
//!    costs nothing, because a resource pack merges per key and the mod's own
//!    English shows through the hole.
//! 5. **Persist as you go, additively.** Accepted keys are held until the next
//!    flush, which re-reads each namespace file and applies only this run's
//!    additions on top. `store::save` rewrites the whole file, and the file is
//!    global rather than instance-scoped — a snapshot written back would
//!    delete an override the editor (or a second run) added meanwhile. An hour
//!    of paid work must not depend on the process surviving to the end, and it
//!    must not cost someone else's work either.
//! 6. **Rebuild the pack, best-effort, always.** A run that wrote every string
//!    but could not rebuild is a success with a caveat, not a failure. So is a
//!    cancelled one, and so is a FAILED one: a provider error at batch 900 of
//!    1000 still leaves 900 batches' worth of verified, paid-for strings on
//!    disk, and throwing the rebuild away would mean the user sees none of
//!    them. The failure is reported on [`RunSummary::failed`] instead of
//!    replacing the summary.
//!
//! Nothing about a failure reaches the launcher log verbatim — see
//! [`transport::log_safe`]. Provider bodies echo the request, and the request
//! carried the user's API key.
//!
//! The split between [`run`] and [`pipeline::execute`] is deliberate:
//! everything that needs a `tauri::AppHandle` (consent, settings, the keychain,
//! paths, the pack rebuild) lives in `run`, and the orchestration itself works
//! on plain data. Integration tests are a separate crate and cannot build an
//! `AppHandle` — no test in `src-tauri/tests/` does — so without this seam the
//! pipeline would be reachable only through the UI.
//!
//! The module is split by what each part owns, not by call order:
//! [`types`] holds what crosses IPC, [`state`] what the run accumulates and
//! persists, [`batch`] one request's worth of work, [`transport`] how a single
//! completion is obtained and what may be said when it fails, and [`pipeline`]
//! the passes that drive them.

mod batch;
#[cfg(test)]
mod fixtures;
mod pipeline;
mod state;
mod transport;
mod types;

pub use batch::{decide_batch, BatchOutcome};
pub use pipeline::execute;
pub use types::{PrefillProgress, Rejected, RunSummary};

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::instances::schema::{AiProvider, GeneralSettings};
use crate::l10n::prefill::cache;
use crate::l10n::prefill::estimate::PrefillEstimate;
use crate::l10n::prefill::glossary::Glossary;
use crate::l10n::store;
use crate::network::consent::AiConsent;

/// In-flight requests to a cloud provider. Nothing else bounds them: the
/// throttle hands an unknown host an effectively unlimited token bucket
/// (`network/throttle.rs`), so this constant is the whole difference between
/// a paced run and a burst that gets the user rate-limited or banned.
pub const CLOUD_CONCURRENCY: usize = 10;

/// In-flight requests to a local model server. Lower on purpose: a local
/// server is usually one process on one GPU, and ten parallel completions
/// there queue behind each other while multiplying peak memory.
pub const LOCAL_CONCURRENCY: usize = 2;

/// Strings per request. The prompt module's ids are dense within a batch, so
/// this is also how many answers one response has to carry.
pub const MAX_BATCH: usize = 40;

/// Sub-directory of the l10n store holding the answer cache, one file per
/// target language. Deliberately NOT inside `<store_dir>/<lang>/`, which
/// `store::namespaces_with_overrides` enumerates as namespace files.
const CACHE_DIR: &str = "prefill-cache";

/// Filename stem for the cache, sanitised by `store::store_path` exactly like
/// a namespace would be.
const CACHE_STEM: &str = "answers";

/// Fill in an instance's missing translations for one language.
///
/// `namespace` scopes the run to a single resource namespace; `None` covers
/// the whole instance. `cancel` is the flag `prefill::cancel::begin` handed
/// the caller — passed in rather than looked up, so the run reads the very
/// flag its own command registered instead of racing the registry. It is an
/// `Arc` because every in-flight batch task holds a clone: a cancel that only
/// the dispatch loop can see still lets ten running batches queue up to
/// `MAX_BATCH` single-string retries each, all decided after the user pressed
/// Cancel.
///
/// `on_progress` is `Send + Sync` because it is held across an `.await`, which
/// makes the returned future `Send` only if it is — and a `#[tauri::command]`
/// can only return a `Send` future. A bare `&dyn Fn(_)` compiles here and
/// fails at the one call site that matters. The bound costs the caller
/// nothing: `tauri::ipc::Channel` is itself `Send + Sync`, so the one-line
/// adapter in `commands::l10n_prefill_start` satisfies it, and so does the
/// `&|_| {}` the unit tests pass.
pub async fn run(
    app: &tauri::AppHandle,
    instance_id: &str,
    lang: &str,
    namespace: Option<&str>,
    cancel: &Arc<AtomicBool>,
    on_progress: &(dyn Fn(PrefillProgress) + Send + Sync),
) -> Result<RunSummary> {
    // The gate. Everything that can reach a model needs this token, and this
    // is the only place one is minted — see the module docs.
    let consent = crate::network::consent::ai_consent(app)?;

    let ctx = resolve_context(app, consent, instance_id, lang, namespace)?;
    let mut summary = pipeline::execute(&ctx, cancel, on_progress).await?;

    on_progress(PrefillProgress {
        done: summary.written,
        total: summary.written,
        phase: types::PHASE_APPLYING.to_string(),
    });
    // Never `?`, and reached even when the run failed or was cancelled: the
    // strings are already on disk, so a failure here is a caveat on whatever
    // the run did buy. `Ok(false)` — pack built, `options.txt` not activated —
    // is the editor's existing, separately-surfaced case and is not a rebuild
    // failure.
    match crate::l10n::apply::rebuild_pack(app, instance_id, lang).await {
        Ok(activated) => {
            summary.pack_rebuilt = true;
            // Carried, not discarded: `false` means the pack is on disk and
            // will not load, which the user has to be told here rather than
            // discover later from an editor banner.
            summary.pack_activated = activated;
        }
        Err(e) => {
            crate::diag!(
                "[l10n] prefill: pack rebuild failed: {}",
                transport::log_safe(&e)
            );
            summary.pack_rebuild_error = Some(e.to_string());
        }
    }
    Ok(summary)
}

/// What a run would do, without doing any of it.
///
/// Resolved through the very same [`resolve_context`] and [`pipeline::discover`]
/// the run uses, and addressed against the same cache via
/// [`state::pipeline_id`]. That is the whole contract of an estimate: numbers
/// produced by a second, parallel derivation would be numbers the run then
/// contradicts, which is worse than showing none.
///
/// It therefore also inherits the run's preflight — the consent token, the API
/// key, the model name. That is deliberate: the failures a run would hit
/// before spending anything are exactly the failures worth surfacing BEFORE
/// the user is shown "1 200 strings, ~40 000 tokens" and asked to confirm.
/// Nothing here reaches the network; the consent token is minted because
/// `resolve_context` carries one, not because a byte is sent.
pub async fn estimate(
    app: &tauri::AppHandle,
    instance_id: &str,
    lang: &str,
    namespace: Option<&str>,
) -> Result<PrefillEstimate> {
    let consent = crate::network::consent::ai_consent(app)?;
    let ctx = resolve_context(app, consent, instance_id, lang, namespace)?;
    let units = pipeline::discover(&ctx).await?;
    Ok(crate::l10n::prefill::estimate::estimate(
        &units,
        &cache::load(&ctx.cache_path),
        &ctx.glossary,
        &state::pipeline_id(&ctx),
        ctx.provider,
    ))
}

/// Everything [`execute`] needs, resolved once from the `AppHandle`.
///
/// `pub` with private fields, for `tests/l10n_prefill_integration.rs`. No
/// integration test in this repository can build a `tauri::AppHandle` —
/// `download_integration.rs`, `worlds_integration.rs` and `servers_diagnose.rs`
/// all say so in their module docs — so [`run`] and [`resolve_context`] are
/// unreachable from one, and the whole orchestration would otherwise be
/// coverable only through the UI. The fields stay private so the only way to
/// build one from outside this module is [`RunContext::for_settings`], which
/// goes through the consent gate.
pub struct RunContext {
    /// Proof the AI-translation permission was on when the run started. Plain
    /// data (zero-sized), so carrying it here keeps the `AppHandle`-free
    /// `execute` seam intact while still gating every provider call.
    consent: AiConsent,
    inst_root: PathBuf,
    store_dir: PathBuf,
    cache_path: PathBuf,
    lang: String,
    namespace: Option<String>,
    provider: AiProvider,
    api_key: Option<String>,
    local_port: u16,
    model: String,
    glossary: Glossary,
}

impl RunContext {
    /// The single place a context is assembled, so the `AppHandle` path
    /// ([`resolve_context`]) and the handle-free one ([`RunContext::for_settings`])
    /// cannot drift. `cache_path` above all: it addresses a file the run and
    /// the estimate must agree about, and two derivations of it would be two
    /// caches.
    fn assemble(
        consent: AiConsent,
        cfg: ProviderConfig,
        inst_root: PathBuf,
        store_dir: PathBuf,
        lang: &str,
        namespace: Option<&str>,
        glossary: Glossary,
    ) -> Self {
        Self {
            consent,
            inst_root,
            cache_path: store::store_path(&store_dir.join(CACHE_DIR), lang, CACHE_STEM),
            store_dir,
            lang: lang.to_string(),
            namespace: namespace.map(str::to_string),
            provider: cfg.provider,
            api_key: cfg.api_key,
            local_port: cfg.local_port,
            model: cfg.model,
            glossary,
        }
    }

    /// A context built from settings rather than from a `tauri::AppHandle` —
    /// the seam `tests/l10n_prefill_integration.rs` drives [`execute`] through.
    /// See [`RunContext`] on why an integration test needs one.
    ///
    /// It is **not** a way around consent. The token is minted by
    /// `network::consent::ai_consent_from`, which is the same function
    /// [`run`]'s `ai_consent` ends in — the only difference is where the
    /// `GeneralSettings` came from. `GeneralSettings::default()` has
    /// `allow_ai_translation: false`, so the default value cannot mint a token
    /// here any more than a default `app.json` can mint one there; a caller
    /// has to state the user's permission as data. `read_app_json` is the only
    /// legitimate source of a `GeneralSettings` in this crate, so supplying a
    /// fabricated one is a deliberate, reviewable act rather than something a
    /// refactor can do by accident.
    ///
    /// `api_key` is passed in rather than read: the keychain is reachable
    /// without an `AppHandle`, but a test must not touch the user's real one.
    #[doc(hidden)]
    pub fn for_settings(
        general: &GeneralSettings,
        api_key: Option<String>,
        inst_root: PathBuf,
        store_dir: PathBuf,
        lang: &str,
        namespace: Option<&str>,
        glossary: Glossary,
    ) -> Result<Self> {
        let consent = crate::network::consent::ai_consent_from(general)?;
        let cfg = ProviderConfig {
            provider: general.ai_provider,
            api_key,
            local_port: general.ai_local_port,
            model: resolve_model(general.ai_provider, &general.ai_model)?,
        };
        Ok(Self::assemble(
            consent, cfg, inst_root, store_dir, lang, namespace, glossary,
        ))
    }
}

/// The model name to send.
///
/// A blank Settings field means "the provider's default" — except for `Local`,
/// whose default is `""` because nothing can enumerate a local server's models
/// offline. Sending an empty model name would make every request fail with
/// whatever that particular server says about it, so this refuses up front and
/// names the setting to fill in.
///
/// The error is `L10nPrefillProvider { status: 0 }` rather than a variant of
/// its own only because `error.rs` is outside this task's blast radius;
/// `status: 0` already means "no usable answer, and it was not the transport",
/// which is exactly this. A dedicated `L10nPrefillModelMissing` variant would
/// read better and is worth adding when that file is next opened.
fn resolve_model(provider: AiProvider, configured: &str) -> Result<String> {
    let configured = configured.trim();
    if !configured.is_empty() {
        return Ok(configured.to_string());
    }
    let default = provider.default_model();
    if default.is_empty() {
        return Err(Error::L10nPrefillProvider {
            provider: provider.id().to_string(),
            status: 0,
            details: "No model name configured. Set Settings → Integrations → \
                      AI translation → Model to a model your local server serves."
                .to_string(),
        });
    }
    Ok(default.to_string())
}

/// Everything needed to reach a model, with nothing instance-specific in it.
///
/// Split out of [`resolve_context`] because the Settings "Test key" button
/// needs exactly this and no instance at all. Re-deriving it there would let
/// the test round-trip a different model, key or port than the run it exists
/// to vouch for — the failure mode being a green "Test key" followed by a run
/// that cannot start.
pub(crate) struct ProviderConfig {
    pub(crate) provider: AiProvider,
    /// `None` only for a provider that needs no key. A cloud provider with no
    /// key stored fails resolution outright rather than arriving as `None`.
    pub(crate) api_key: Option<String>,
    pub(crate) local_port: u16,
    pub(crate) model: String,
}

/// The API key filed for `provider`, or `None` when nothing usable is stored.
///
/// Trimmed and emptiness-checked HERE, once, so that "is a key stored?" and
/// "which key will the run send?" can never disagree — a status that reported
/// a whitespace-only entry as present would send the user back to Settings
/// with nothing visibly wrong to fix.
fn stored_key(provider: AiProvider) -> Result<Option<String>> {
    Ok(
        crate::accounts::keychain::retrieve(&crate::accounts::keychain::ai_provider_key(
            provider.id(),
        ))?
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty()),
    )
}

/// Whether a usable API key is stored for `provider`.
///
/// The command layer calls this rather than [`stored_key`] so the secret is
/// never bound to a variable in a file whose whole job is to return values to
/// the UI. The key status crosses IPC; the key never can.
pub(crate) fn has_stored_key(provider: AiProvider) -> Result<bool> {
    Ok(stored_key(provider)?.is_some())
}

/// Read the AI settings and resolve the model and credential they imply.
pub(crate) fn resolve_provider(app: &tauri::AppHandle) -> Result<ProviderConfig> {
    let app_file = crate::paths::app_file(app).map_err(|e| Error::io("<app_file>", e))?;
    let general = crate::instances::store::read_app_json(&app_file)?.general;
    let provider = general.ai_provider;
    let model = resolve_model(provider, &general.ai_model)?;

    let api_key = if provider.needs_key() {
        Some(
            stored_key(provider)?.ok_or_else(|| Error::L10nPrefillKeyMissing {
                provider: provider.id().to_string(),
            })?,
        )
    } else {
        None
    };

    Ok(ProviderConfig {
        provider,
        api_key,
        local_port: general.ai_local_port,
        model,
    })
}

fn resolve_context(
    app: &tauri::AppHandle,
    consent: AiConsent,
    instance_id: &str,
    lang: &str,
    namespace: Option<&str>,
) -> Result<RunContext> {
    // Settings, model and credential first, exactly as before the split: the
    // cheap failures that need no instance at all are the ones worth hitting
    // before any jar is opened.
    let cfg = resolve_provider(app)?;

    let inst_root = crate::commands::instance_root(app, instance_id)?;
    let store_dir = crate::paths::l10n_dir(app).map_err(|e| Error::io("<l10n_dir>", e))?;
    let versions_dir =
        crate::paths::versions_dir(app).map_err(|e| Error::io("<versions_dir>", e))?;
    let assets_dir = crate::paths::assets_dir(app).map_err(|e| Error::io("<assets_dir>", e))?;
    let (mc_version, _loader) = crate::commands::read_active_mc_and_loader(app, instance_id)?;

    let client_jar = crate::datapacks::compat::client_jar_path(&versions_dir, &mc_version);
    // There is no helper for the version JSON path in this repo; the formula
    // is `versions_dir/<id>/<id>.json` with the RAW mc_version, the same value
    // `client_jar_path` takes.
    let version_json = versions_dir
        .join(&mc_version)
        .join(format!("{mc_version}.json"));
    let glossary = crate::l10n::prefill::glossary::load_for_instance(
        &client_jar,
        &assets_dir,
        &version_json,
        lang,
        &mc_version,
    );

    Ok(RunContext::assemble(
        consent, cfg, inst_root, store_dir, lang, namespace, glossary,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrency_limits_are_bounded_and_lower_for_a_local_model() {
        // Nothing but this constant stops a burst at a provider: unknown
        // hosts get an effectively unlimited token bucket in the throttle.
        assert!(CLOUD_CONCURRENCY <= 16);
        assert!(LOCAL_CONCURRENCY < CLOUD_CONCURRENCY);
        assert!(LOCAL_CONCURRENCY >= 1);
    }

    // -----------------------------------------------------------------
    // model resolution
    // -----------------------------------------------------------------

    #[test]
    fn a_local_provider_with_no_model_names_the_setting_instead_of_sending_nothing() {
        // `AiProvider::Local.default_model()` is "" — nothing can enumerate a
        // local server's models offline. Falling back to it would put an empty
        // model name in the request body and fail with whatever that server
        // happens to say about it.
        let err = resolve_model(AiProvider::Local, "  ").expect_err("no model to fall back to");
        match err {
            Error::L10nPrefillProvider {
                provider, details, ..
            } => {
                assert_eq!(provider, "local");
                assert!(
                    details.contains("Model"),
                    "must name the setting: {details}"
                );
            }
            other => panic!("expected a typed provider error, got {other:?}"),
        }
    }

    #[test]
    fn a_blank_model_falls_back_to_the_cloud_default_and_a_set_one_wins() {
        assert_eq!(
            resolve_model(AiProvider::Anthropic, "").expect("cloud has a default"),
            AiProvider::Anthropic.default_model()
        );
        assert_eq!(
            resolve_model(AiProvider::Local, " my-local-model ").expect("configured"),
            "my-local-model"
        );
    }
}

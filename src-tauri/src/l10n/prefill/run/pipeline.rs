//! The pipeline itself, over plain data: discovery, the free-win pass, and the
//! concurrent dispatch of one role's batches at a time.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::instances::schema::AiProvider;
use crate::l10n::prefill::plan::{build_batches, Batch, PrefillUnit};
use crate::l10n::prefill::role::UiRole;
use crate::l10n::store::{self, KeyState};

use super::batch::translate_batch;
use super::state::{fold_pass, take_free_wins, RunState};
use super::transport::is_cancelled;
use super::types::{clamp_u32, phase_of, PrefillProgress, RunSummary, PHASE_FREE, PHASE_SCANNING};
use super::{RunContext, CLOUD_CONCURRENCY, LOCAL_CONCURRENCY, MAX_BATCH};

/// Completed batches between flushes. One flush per batch would rewrite every
/// touched namespace file hundreds of times over a pack-sized run; one flush
/// at the end would put an hour of paid work at the mercy of a crash. Eight is
/// the bound on how much a kill can cost.
const FLUSH_EVERY_BATCHES: usize = 8;

/// The pipeline itself, over plain data. Takes no `AppHandle` — see the module
/// docs on why that seam exists.
pub(super) async fn execute(
    ctx: &RunContext,
    cancel: &Arc<AtomicBool>,
    on_progress: &dyn Fn(PrefillProgress),
) -> Result<RunSummary> {
    on_progress(PrefillProgress {
        done: 0,
        total: 0,
        phase: PHASE_SCANNING.to_string(),
    });

    // The one genuinely pre-spend failure: nothing has been written, nothing
    // has been bought, and there is no partial summary worth reporting.
    let units = discover(ctx).await?;
    Ok(run_pipeline(ctx, units, cancel, on_progress).await)
}

/// Everything after discovery. Split out so the whole of it — free wins,
/// passes, flush, failure accounting — is reachable from a unit test without a
/// mods folder to scan.
///
/// Returns a summary rather than a `Result` on purpose: past this point the
/// run can have written keys, and a failure must be reported ON the summary so
/// the caller still rebuilds the pack around what was bought.
async fn run_pipeline(
    ctx: &RunContext,
    units: Vec<PrefillUnit>,
    cancel: &Arc<AtomicBool>,
    on_progress: &dyn Fn(PrefillProgress),
) -> RunSummary {
    let batches = build_batches(&units, MAX_BATCH);

    let mut state = RunState::new(ctx);
    state.total = clamp_u32(batches.iter().map(|b| b.units.len()).sum::<usize>());

    // Free wins first — cache, then vanilla — so the paid passes only ever see
    // what nothing on disk could already answer.
    let pending = take_free_wins(ctx, &mut state, batches);
    on_progress(PrefillProgress {
        done: state.done,
        total: state.total,
        phase: PHASE_FREE.to_string(),
    });
    if let Err(e) = state.flush(ctx) {
        state.note_failure(&e);
        return state.summary;
    }

    for (role, group) in into_passes(pending) {
        if state.summary.cancelled || state.summary.failed.is_some() {
            break;
        }
        if let Err(e) = run_pass(ctx, &mut state, role, group, cancel, on_progress).await {
            state.note_failure(&e);
        }
        fold_pass(&mut state, role);
    }

    if let Err(e) = state.flush(ctx) {
        state.note_failure(&e);
    }
    state.summary
}

/// Every key this run could fill.
///
/// One pass over the jars, not one per namespace: a 150-mod pack has
/// comparably many namespaces, and asking per namespace would re-read every
/// jar that many times.
///
/// The namespace stores loaded here are used only to classify keys and are
/// then dropped. Holding them for the whole run and saving them back is
/// exactly the stale-snapshot bug `PendingWrite` exists to avoid.
async fn discover(ctx: &RunContext) -> Result<Vec<PrefillUnit>> {
    let lang_maps =
        crate::l10n::namespace_scan::instance_lang_maps(&ctx.inst_root, &ctx.lang).await?;

    let mut units = Vec::new();
    for (namespace, (en, mod_tr)) in &lang_maps {
        if ctx
            .namespace
            .as_deref()
            .is_some_and(|want| want != namespace)
        {
            continue;
        }
        let store = store::load(&ctx.store_dir, &ctx.lang, namespace);
        for row in store::namespace_key_rows(&store, en, Some(mod_tr)) {
            // A key whose English value is blank is a real, present key in
            // Minecraft lang files, and it lands in `Missing` too. It can
            // never yield a valid translation, so it must not burn a slot.
            if row.state != KeyState::Missing || row.source_en.trim().is_empty() {
                continue;
            }
            units.push(PrefillUnit {
                namespace: namespace.clone(),
                key: row.key,
                source_en: row.source_en,
            });
        }
    }
    Ok(units)
}

/// Split the batch list into consecutive same-role passes. `build_batches`
/// already emits `Name`, then `Prose`, then `Other`, so this only has to
/// respect that order rather than impose one.
fn into_passes(batches: Vec<Batch>) -> Vec<(UiRole, Vec<Batch>)> {
    let mut passes: Vec<(UiRole, Vec<Batch>)> = Vec::new();
    for batch in batches {
        match passes.last_mut() {
            Some((role, group)) if *role == batch.role => group.push(batch),
            _ => passes.push((batch.role, vec![batch])),
        }
    }
    passes
}

/// One role's worth of batches, dispatched concurrently and absorbed as they
/// land.
///
/// Two bounds, doing two different jobs. `JoinSet::len()` caps how many
/// batches this run has in flight, which is what lets results be absorbed and
/// flushed while later batches are still queued — cancellation only means
/// something if there are batches left to not dispatch. The process-wide
/// semaphore inside [`provider_slots`] caps how many requests reach the
/// provider across ALL runs, which per-run bounding cannot do.
async fn run_pass(
    ctx: &RunContext,
    state: &mut RunState,
    role: UiRole,
    batches: Vec<Batch>,
    cancel: &Arc<AtomicBool>,
    on_progress: &dyn Fn(PrefillProgress),
) -> Result<()> {
    let limit = concurrency_for(ctx.provider);
    let mut queue = batches.into_iter();
    let mut tasks = tokio::task::JoinSet::new();
    let mut since_flush = 0usize;
    let mut failure: Option<Error> = None;

    loop {
        while tasks.len() < limit && failure.is_none() {
            // The top of a dispatch: past this point the request is the
            // provider's problem, so this is where a cancel has to be seen.
            // It is NOT the only place — a batch already in flight carries its
            // own clone of the flag (`BatchJob::cancel`).
            if is_cancelled(cancel) {
                state.summary.cancelled = true;
            }
            if state.summary.cancelled {
                break;
            }
            let Some(batch) = queue.next() else {
                break;
            };
            let job = state.job_for(ctx, role, batch, cancel);
            tasks.spawn(async move {
                // Held for the request's whole life. `acquire` can only fail
                // on a closed semaphore, and these buckets are `static` and
                // never closed — binding the `Result` keeps the permit alive
                // for the scope either way.
                let _permit = provider_slots(job.provider).acquire().await;
                translate_batch(job).await
            });
        }

        let Some(joined) = tasks.join_next().await else {
            break;
        };
        match joined {
            Ok(Ok(output)) => state.absorb(output),
            Ok(Err(e)) => {
                failure.get_or_insert(e);
            }
            Err(e) => {
                failure.get_or_insert(Error::L10nPrefillProvider {
                    provider: ctx.provider.id().to_string(),
                    status: 0,
                    details: format!("a translation batch did not finish: {e}"),
                });
            }
        }

        since_flush += 1;
        if since_flush >= FLUSH_EVERY_BATCHES {
            // No `?`: an early return here would abandon the batches still in
            // flight and skip the final flush that would have retried the
            // namespaces this one could not write.
            if let Err(e) = state.flush(ctx) {
                failure.get_or_insert(e);
            }
            since_flush = 0;
        }
        on_progress(PrefillProgress {
            done: state.done,
            total: state.total,
            phase: phase_of(role).to_string(),
        });
    }

    // Whatever landed before the failure was paid for and verified; persist it
    // before surfacing the error, so the next run starts from it.
    if let Err(e) = state.flush(ctx) {
        failure.get_or_insert(e);
    }
    match failure {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn concurrency_for(provider: AiProvider) -> usize {
    match provider {
        AiProvider::Local => LOCAL_CONCURRENCY,
        AiProvider::Anthropic | AiProvider::Gemini | AiProvider::Groq => CLOUD_CONCURRENCY,
    }
}

/// Process-wide request slots, one bucket per provider class. Static, like
/// `network::consent`'s dial slots, so two instances pre-filling at once share
/// one budget instead of doubling the burst.
fn provider_slots(provider: AiProvider) -> &'static tokio::sync::Semaphore {
    static CLOUD: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    static LOCAL: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    match provider {
        AiProvider::Local => LOCAL.get_or_init(|| tokio::sync::Semaphore::new(LOCAL_CONCURRENCY)),
        AiProvider::Anthropic | AiProvider::Gemini | AiProvider::Groq => {
            CLOUD.get_or_init(|| tokio::sync::Semaphore::new(CLOUD_CONCURRENCY))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{test_ctx_with, vanilla};
    use super::*;

    #[tokio::test]
    async fn a_run_that_cannot_write_still_reports_what_it_bought() {
        // The regression: `execute` used to `?` the error out, which threw the
        // summary away AND skipped the pack rebuild in `run`. Rotate a key at
        // batch 900 of 1000 and ~36 000 verified keys sit on disk while the
        // game shows nothing new and the error says nothing about it.
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx_with(dir.path(), vanilla(&[("Copper Ingot", "Медный слиток")]));

        // Vanilla answers every unit, so the pipeline runs end to end with no
        // provider involved at all.
        let units = vec![PrefillUnit {
            namespace: "create".to_string(),
            key: "item.create.a".to_string(),
            source_en: "Copper Ingot".to_string(),
        }];
        let poisoned = store::store_path(&ctx.store_dir, &ctx.lang, "create");
        std::fs::create_dir_all(&poisoned).expect("poison the destination");

        let cancel = Arc::new(AtomicBool::new(false));
        let summary = run_pipeline(&ctx, units, &cancel, &|_| {}).await;

        assert_eq!(summary.from_glossary, 1);
        assert_eq!(summary.written, 1);
        assert!(
            summary.failed.is_some(),
            "a failure must be reported ON the summary, not returned in place of it"
        );
        assert!(!summary.cancelled);
    }
}

//! What a run accumulates and how it reaches disk: the pending writes, the
//! answer cache, the frozen termbase, and the free wins that never cost
//! anything.

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::l10n::prefill::cache::{self, cache_key, CacheMap, PipelineId};
use crate::l10n::prefill::glossary::Glossary;
use crate::l10n::prefill::plan::{Batch, Target};
use crate::l10n::prefill::prompt::PROMPT_VERSION;
use crate::l10n::prefill::role::UiRole;
use crate::l10n::prefill::verify;
use crate::l10n::store::{self, Origin};

use super::batch::{BatchJob, BatchOutput};
use super::types::RunSummary;
use super::RunContext;

/// Term pairs injected into one prompt. A termbase that outgrows the batch it
/// guides costs more than the translation and buries the terms that matter.
const MAX_GLOSSARY_TERMS: usize = 24;

/// One accepted key, held until the next flush.
///
/// Deliberately not a `NamespaceStore` snapshot: `store::save` rewrites the
/// whole file, so keeping a store in memory across a run and saving it back
/// would delete every entry anyone else wrote in the meantime. What the run
/// owns is its own ADDITIONS; the file on disk is the base, re-read at each
/// flush.
struct PendingWrite {
    key: String,
    value: String,
    source_en: String,
    /// Captured when the answer was accepted, not when it reached disk, so a
    /// flush that is retried later does not backdate or postdate the entry.
    updated_at: f64,
}

/// Mutable state threaded through the passes. A struct rather than a dozen
/// `&mut` parameters, so each pass reads as what it does rather than as its
/// plumbing.
pub(super) struct RunState {
    pipeline: PipelineId,
    /// This run's own additions, per namespace, not yet on disk. Cleared per
    /// namespace as each one saves; a namespace whose save failed keeps its
    /// entries so a later flush retries them.
    pending: BTreeMap<String, Vec<PendingWrite>>,
    cache: CacheMap,
    /// Names decided in pass 1 — whether by a model, the answer cache or
    /// vanilla — frozen as this run's own termbase for the later passes. Empty
    /// DURING a model name pass on purpose: folding a name into the very pass
    /// that produced it would make the output depend on which batch happened
    /// to finish first. The free wins ARE seeded immediately, because they are
    /// all decided before any model pass starts.
    learned: BTreeMap<String, String>,
    /// The current pass's accepted answers, folded into `learned` when a name
    /// pass ends.
    pass_answers: BTreeMap<String, String>,
    pub(super) summary: RunSummary,
    pub(super) done: u32,
    pub(super) total: u32,
}

impl RunState {
    pub(super) fn new(ctx: &RunContext) -> Self {
        Self {
            pipeline: PipelineId {
                target_lang: ctx.lang.clone(),
                model: ctx.model.clone(),
                prompt_version: PROMPT_VERSION,
                glossary_version: ctx.glossary.version().to_string(),
            },
            pending: BTreeMap::new(),
            cache: cache::load(&ctx.cache_path),
            learned: BTreeMap::new(),
            pass_answers: BTreeMap::new(),
            summary: RunSummary::new(),
            done: 0,
            total: 0,
        }
    }

    /// Record the failure that stopped the run. The FIRST one is kept: later
    /// ones are usually its consequences, and the first is what the user has
    /// to act on.
    pub(super) fn note_failure(&mut self, error: &Error) {
        if self.summary.failed.is_none() {
            self.summary.failed = Some(error.to_string());
        }
    }

    /// Record one accepted translation against the key it belongs to, to be
    /// applied at the next flush. Always `Origin::Machine` — the hand-edit
    /// path is `set`, which stays `Manual`, so a user editing a machine string
    /// reclaims it.
    fn write(&mut self, target: &Target, value: &str, source_en: &str) {
        self.pending
            .entry(target.namespace.clone())
            .or_default()
            .push(PendingWrite {
                key: target.key.clone(),
                value: value.to_string(),
                source_en: source_en.to_string(),
                updated_at: crate::instances::unix_ms_f64(),
            });
        self.summary.written += 1;
    }

    /// Persist every namespace touched since the last flush, plus the cache.
    ///
    /// Each namespace is **re-read** and this run's additions applied on top.
    /// `store::save` rewrites the whole file and `<store_dir>/<lang>/<ns>.json`
    /// is global rather than instance-scoped, so writing back a snapshot taken
    /// at discovery would delete anything the editor's own save — or a second
    /// instance's run — put there in between. Deleting a `Manual` override
    /// would destroy exactly the reclaim `Origin::Manual` exists for.
    ///
    /// Every namespace is attempted even after one fails, and a namespace that
    /// failed keeps its entries so a later flush retries it: on Windows
    /// `fs::rename` returns `os error 5` whenever anything holds the
    /// destination open, and an on-access scanner is enough to cause that.
    /// Draining the set up front would silently discard every namespace after
    /// the first failure, plus the cache — an hour of paid work.
    ///
    /// The first error is returned, last. A cache write that fails is only
    /// logged: the cache is an optimisation, and its worst case is that the
    /// next run re-earns the entries.
    pub(super) fn flush(&mut self, ctx: &RunContext) -> Result<()> {
        let mut first_error: Option<Error> = None;
        let mut retained: BTreeMap<String, Vec<PendingWrite>> = BTreeMap::new();

        for (namespace, writes) in std::mem::take(&mut self.pending) {
            let mut store = store::load(&ctx.store_dir, &ctx.lang, &namespace);
            for w in &writes {
                store.set_with_origin(
                    w.key.as_str(),
                    w.value.as_str(),
                    w.source_en.as_str(),
                    w.updated_at,
                    Origin::Machine,
                );
            }
            if let Err(io_err) = store::save(&ctx.store_dir, &store) {
                crate::diag!("[l10n] prefill: could not save namespace {namespace}: {io_err}");
                first_error.get_or_insert(Error::io("<l10n override store>", io_err));
                retained.insert(namespace, writes);
            }
        }
        self.pending = retained;

        if let Err(io_err) = cache::save(&ctx.cache_path, &self.cache) {
            crate::diag!("[l10n] prefill: could not save the answer cache: {io_err}");
        }

        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    pub(super) fn job_for(
        &self,
        ctx: &RunContext,
        role: UiRole,
        batch: Batch,
        cancel: &Arc<AtomicBool>,
    ) -> BatchJob {
        let sources: Vec<String> = batch.units.iter().map(|u| u.source_en.clone()).collect();
        BatchJob {
            consent: ctx.consent,
            provider: ctx.provider,
            api_key: ctx.api_key.clone(),
            local_port: ctx.local_port,
            model: ctx.model.clone(),
            lang: ctx.lang.clone(),
            role,
            terms: batch_terms(&ctx.glossary, &self.learned, &sources),
            units: batch.units,
            cancel: Arc::clone(cancel),
        }
    }

    pub(super) fn absorb(&mut self, output: BatchOutput) {
        for written in &output.writes {
            self.write(
                &Target {
                    namespace: written.namespace.clone(),
                    key: written.key.clone(),
                },
                &written.value,
                &written.source_en,
            );
        }
        for (source_en, value) in output.answers {
            self.cache.insert(
                cache_key(&source_en, output.role, &self.pipeline),
                value.clone(),
            );
            self.pass_answers.insert(source_en, value);
        }
        self.summary.rejected += output.rejected;
        self.summary.prompt_tokens = self
            .summary
            .prompt_tokens
            .saturating_add(output.prompt_tokens);
        self.summary.completion_tokens = self
            .summary
            .completion_tokens
            .saturating_add(output.completion_tokens);
        self.summary.usage_known &= output.usage_known;
        self.done = self.done.saturating_add(output.resolved);
    }
}

/// Fold a finished pass's accepted answers into the run's frozen termbase.
///
/// `extend`, never `=`: [`take_free_wins`] already seeded `learned` with every
/// Name it answered from the cache or from vanilla, and those answers never
/// pass through `absorb`. Assigning would drop them, and the prose pass would
/// re-coin terms the run had already settled — two translations of one name,
/// both accepted by the verifier, and a run that reports success.
pub(super) fn fold_pass(state: &mut RunState, role: UiRole) {
    let answers = std::mem::take(&mut state.pass_answers);
    if role == UiRole::Name {
        state.learned.extend(answers);
    }
}

/// Answer everything the cache or vanilla already knows, and hand back only
/// the batches with work left in them.
pub(super) fn take_free_wins(
    ctx: &RunContext,
    state: &mut RunState,
    batches: Vec<Batch>,
) -> Vec<Batch> {
    let mut pending = Vec::new();
    for batch in batches {
        let mut remaining = Vec::new();
        for unit in batch.units {
            let Some((value, source)) = free_answer(ctx, state, batch.role, &unit.source_en) else {
                remaining.push(unit);
                continue;
            };
            match source {
                FreeSource::Cache => state.summary.from_cache += 1,
                FreeSource::Glossary => state.summary.from_glossary += 1,
            }
            // A free NAME is still a name this run has settled on, and the
            // termbase is what stops the prose pass coining a second word for
            // it. `absorb` never sees these — a unit answered here is consumed
            // before any model is involved — so if the seeding does not happen
            // here it never happens at all, and a run whose names are all
            // cache hits carries an EMPTY termbase into the prose pass.
            //
            // Sound to freeze immediately: every free win is decided before
            // the first model pass starts, so there is no ordering to depend
            // on.
            if batch.role == UiRole::Name {
                state.learned.insert(unit.source_en.clone(), value.clone());
            }
            for target in &unit.targets {
                state.write(target, &value, &unit.source_en);
            }
            state.done += 1;
        }
        if !remaining.is_empty() {
            pending.push(Batch {
                role: batch.role,
                units: remaining,
            });
        }
    }
    pending
}

enum FreeSource {
    Cache,
    Glossary,
}

/// A translation that costs nothing: a previous run's cached answer, or
/// vanilla's own translation of the identical string.
///
/// Both are re-verified rather than trusted. The cache is an ordinary file on
/// disk that a user can edit or a bad sector can corrupt, and "never write a
/// string you cannot verify" is only a guarantee if it holds unconditionally.
/// A free answer that fails verification is dropped rather than counted, so
/// the string simply goes to the model like any other.
fn free_answer(
    ctx: &RunContext,
    state: &RunState,
    role: UiRole,
    source_en: &str,
) -> Option<(String, FreeSource)> {
    if let Some(hit) = state
        .cache
        .get(&cache_key(source_en, role, &state.pipeline))
    {
        if verify::verify(source_en, hit).is_ok() {
            return Some((hit.clone(), FreeSource::Cache));
        }
    }
    let vanilla = ctx.glossary.exact(source_en)?;
    verify::verify(source_en, &vanilla)
        .is_ok()
        .then_some((vanilla, FreeSource::Glossary))
}

/// The term pairs for one batch: this run's own frozen names first, then
/// vanilla's for whatever budget is left. Both are restricted to terms that
/// actually occur in the batch — a prompt carrying a whole termbase buries the
/// handful that matter.
///
/// Learned first is not a preference, it is the only order that works.
/// `Glossary::terms_for` matches raw lowercase SUBSTRINGS with no word
/// boundary, over a termbase holding every vanilla entry of three words or
/// fewer: `on` matches inside `stone`, `iron` inside `environment`. A
/// forty-sentence prose batch therefore fills the whole budget with vanilla
/// noise, and under the reverse order not one learned term ever reached the
/// prompt — which is the entire point of the two-pass design. Vanilla's
/// whole-string matches are already served for free by `Glossary::exact`,
/// whereas a mod-coined name exists nowhere but `learned`.
fn batch_terms(
    glossary: &Glossary,
    learned: &BTreeMap<String, String>,
    sources: &[String],
) -> Vec<(String, String)> {
    let haystacks: Vec<String> = sources.iter().map(|s| s.to_lowercase()).collect();
    let mut out: Vec<(String, String)> = Vec::new();

    for (en, tr) in learned {
        if out.len() >= MAX_GLOSSARY_TERMS {
            break;
        }
        let needle = en.to_lowercase();
        if haystacks.iter().any(|h| h.contains(&needle)) {
            out.push((en.clone(), tr.clone()));
        }
    }

    for (en, tr) in glossary.terms_for(sources, MAX_GLOSSARY_TERMS) {
        if out.len() >= MAX_GLOSSARY_TERMS {
            break;
        }
        if !out.iter().any(|(existing, _)| existing == en) {
            out.push((en.to_string(), tr.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l10n::prefill::plan::BatchUnit;
    use crate::l10n::scan::LangMap;

    use super::super::fixtures::{test_ctx, test_ctx_with, vanilla};

    fn target(namespace: &str, key: &str) -> Target {
        Target {
            namespace: namespace.to_string(),
            key: key.to_string(),
        }
    }

    // -----------------------------------------------------------------
    // termbase assembly
    // -----------------------------------------------------------------

    #[test]
    fn a_learned_term_is_never_crowded_out_by_vanilla_substring_matches() {
        // `Glossary::terms_for` matches raw lowercase substrings with no word
        // boundary, over a termbase holding every vanilla entry of three words
        // or fewer — "on" hits inside "stone", "iron" inside "environment". A
        // single prose batch therefore fills the whole budget with vanilla
        // noise, and filling from vanilla FIRST meant not one learned term
        // ever reached the prompt: the two-pass design silently did nothing.
        // The old test could not see this because it passed an empty glossary.
        let filler: Vec<String> = (0..30).map(|i| format!("wid{i:02}")).collect();
        let mut en = LangMap::new();
        let mut tr = LangMap::new();
        for (i, word) in filler.iter().enumerate() {
            en.insert(format!("k{i}"), word.clone());
            tr.insert(format!("k{i}"), format!("сл{i:02}"));
        }
        let glossary = Glossary::from_lang_maps(&en, &tr, "1.20.1");

        let learned = BTreeMap::from([(
            "Andesite Alloy".to_string(),
            "Андезитовый сплав".to_string(),
        )]);
        let sources = vec![format!(
            "Craft an Andesite Alloy using {}",
            filler.join(" ")
        )];

        let terms = batch_terms(&glossary, &learned, &sources);
        assert_eq!(
            terms.len(),
            MAX_GLOSSARY_TERMS,
            "vanilla must still fill the remaining budget"
        );
        assert_eq!(
            terms[0],
            (
                "Andesite Alloy".to_string(),
                "Андезитовый сплав".to_string()
            ),
            "a mod-coined name exists ONLY in `learned`; vanilla's whole-string \
             matches are already served free by Glossary::exact"
        );
    }

    #[test]
    fn a_batch_learns_this_runs_own_names_for_terms_vanilla_never_had() {
        // The whole point of the two passes: a name decided in pass 1 is
        // frozen and shown to the prose pass, so one term does not become
        // three phrases across a mod's tooltips.
        let learned = BTreeMap::from([
            (
                "Andesite Alloy".to_string(),
                "Андезитовый сплав".to_string(),
            ),
            ("Cogwheel".to_string(), "Шестерня".to_string()),
        ]);
        let sources = vec!["Craft an Andesite Alloy to begin".to_string()];
        let terms = batch_terms(&Glossary::empty(), &learned, &sources);
        assert_eq!(
            terms,
            vec![(
                "Andesite Alloy".to_string(),
                "Андезитовый сплав".to_string()
            )],
            "only terms that occur in the batch may be injected"
        );
    }

    // -----------------------------------------------------------------
    // flushing: additive, and never all-or-nothing
    // -----------------------------------------------------------------

    #[test]
    fn a_flush_keeps_an_override_written_while_the_run_was_in_flight() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx(dir.path());
        let mut state = RunState::new(&ctx);

        state.write(&target("create", "item.create.a"), "Медь", "Copper");
        state.flush(&ctx).expect("first flush");

        // Meanwhile the user fixes a machine string in the editor.
        // `l10n_set_override` does its own load → set → save against the very
        // same global file — nothing is instance-scoped here.
        let mut disk = store::load(&ctx.store_dir, &ctx.lang, "create");
        disk.set("item.create.fixed", "Исправлено", "Fixed", 1.0);
        store::save(&ctx.store_dir, &disk).expect("editor save");

        state.write(&target("create", "item.create.b"), "Железо", "Iron");
        state.flush(&ctx).expect("second flush");

        let after = store::load(&ctx.store_dir, &ctx.lang, "create");
        assert!(
            after.entries.contains_key("item.create.fixed"),
            "the flush wrote back a snapshot older than the file and destroyed \
             the reclaim Origin::Manual exists for"
        );
        assert!(after.entries.contains_key("item.create.a"));
        assert!(after.entries.contains_key("item.create.b"));
    }

    #[test]
    fn a_namespace_that_cannot_be_written_costs_neither_the_others_nor_itself() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx(dir.path());
        let mut state = RunState::new(&ctx);

        // Poison the FIRST namespace alphabetically: a directory where the
        // store file has to go makes the atomic rename fail. On Windows the
        // real cause is an on-access scanner holding the destination open
        // (os error 5), which is not exotic at all.
        let poisoned = store::store_path(&ctx.store_dir, &ctx.lang, "aaa");
        std::fs::create_dir_all(&poisoned).expect("poison the destination");

        state.write(&target("aaa", "item.a"), "А", "A");
        state.write(&target("zzz", "item.z"), "Я", "Z");
        state.cache.insert("k".to_string(), "Значение".to_string());

        let err = state
            .flush(&ctx)
            .expect_err("the poisoned namespace must surface");
        assert!(matches!(err, Error::Io { .. }), "got {err:?}");

        let zzz = store::load(&ctx.store_dir, &ctx.lang, "zzz");
        assert!(
            zzz.entries.contains_key("item.z"),
            "one failing namespace must not discard every namespace after it"
        );
        assert_eq!(
            cache::load(&ctx.cache_path).get("k").map(String::as_str),
            Some("Значение"),
            "the cache must be written even when a namespace failed"
        );

        // And the failure is retried rather than dropped on the floor.
        std::fs::remove_dir(&poisoned).expect("un-poison");
        state.flush(&ctx).expect("retry");
        let aaa = store::load(&ctx.store_dir, &ctx.lang, "aaa");
        assert!(
            aaa.entries.contains_key("item.a"),
            "a namespace whose save failed must stay pending for a later flush"
        );
    }

    // -----------------------------------------------------------------
    // the termbase the two-pass design depends on
    // -----------------------------------------------------------------

    #[test]
    fn a_name_answered_for_free_still_enters_the_frozen_termbase() {
        // A unit served by `take_free_wins` never reaches `absorb`, which is
        // the only other place `pass_answers` is written. If every Name is a
        // cache or glossary hit, `into_passes` yields no Name group at all —
        // so unless the seeding happens here the termbase stays EMPTY, the
        // prose pass re-coins the term, and `verify` happily accepts the
        // second wording.
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx_with(dir.path(), vanilla(&[("Copper Ingot", "Медный слиток")]));
        let mut state = RunState::new(&ctx);

        let batches = vec![Batch {
            role: UiRole::Name,
            units: vec![BatchUnit {
                id: "s0".to_string(),
                source_en: "Copper Ingot".to_string(),
                targets: vec![target("create", "item.create.a")],
            }],
        }];
        let pending = take_free_wins(&ctx, &mut state, batches);

        assert!(pending.is_empty(), "vanilla already knows this string");
        assert_eq!(state.summary.from_glossary, 1);
        assert_eq!(
            state.learned.get("Copper Ingot").map(String::as_str),
            Some("Медный слиток"),
            "a name the run never had to ask for is still a name the run settled on"
        );
    }

    #[test]
    fn a_free_prose_answer_is_not_promoted_to_a_term() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx_with(dir.path(), vanilla(&[("Hold shift", "Удерживайте Shift")]));
        let mut state = RunState::new(&ctx);

        let batches = vec![Batch {
            role: UiRole::Prose,
            units: vec![BatchUnit {
                id: "s0".to_string(),
                source_en: "Hold shift".to_string(),
                targets: vec![target("create", "create.tooltip.x")],
            }],
        }];
        take_free_wins(&ctx, &mut state, batches);
        assert!(
            state.learned.is_empty(),
            "a sentence is not a term; only the Name pass feeds the termbase"
        );
    }

    #[test]
    fn folding_a_name_pass_extends_the_termbase_rather_than_replacing_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx(dir.path());
        let mut state = RunState::new(&ctx);
        state
            .learned
            .insert("Copper Ingot".to_string(), "Медный слиток".to_string());
        state.pass_answers.insert(
            "Andesite Alloy".to_string(),
            "Андезитовый сплав".to_string(),
        );

        fold_pass(&mut state, UiRole::Name);

        assert_eq!(
            state.learned.len(),
            2,
            "assigning the pass's answers would drop every free win seeded before it"
        );
        assert!(state.pass_answers.is_empty());

        // A prose pass consumes the termbase; it does not add to it.
        state
            .pass_answers
            .insert("Hold shift".to_string(), "Удерживайте Shift".to_string());
        fold_pass(&mut state, UiRole::Prose);
        assert_eq!(state.learned.len(), 2);
    }

    #[test]
    fn only_the_first_failure_is_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx(dir.path());
        let mut state = RunState::new(&ctx);
        state.note_failure(&Error::L10nPrefillBusy);
        state.note_failure(&Error::L10nPrefillKeyMissing {
            provider: "groq".to_string(),
        });
        let failed = state.summary.failed.expect("a failure was noted");
        assert!(
            failed.contains("already running"),
            "the first failure is the cause; the rest are its consequences: {failed}"
        );
    }
}

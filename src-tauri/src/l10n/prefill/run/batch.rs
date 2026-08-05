//! One request's worth of work: what is sent, what the answer is judged to be
//! worth, and the single retry a refused string gets.

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::error::Result;
use crate::instances::schema::AiProvider;
use crate::l10n::prefill::plan::{BatchUnit, Target};
use crate::l10n::prefill::prompt;
use crate::l10n::prefill::provider;
use crate::l10n::prefill::role::UiRole;
use crate::l10n::prefill::verify::{self, RejectReason};
use crate::network::consent::AiConsent;

use super::transport::{complete_with_retry, is_cancelled, log_safe};
use super::types::{clamp_u32, Rejected};

/// How much of an unusable answer reaches the diagnostic log. Mirrors the cap
/// `provider::DETAILS_CAP` puts on a quoted body: enough to see the shape of
/// what came back, not enough for a chatty model to flood the log.
const RAW_ANSWER_LOG_CAP: usize = 400;

/// What [`decide_batch`] made of one response. Not an IPC type: `Target` is
/// internal plumbing, and the UI is told counts, not fan-out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BatchOutcome {
    /// One entry per KEY — the fan-out of an accepted answer across every
    /// namespace and key that was waiting on that English string.
    pub accepted: Vec<(Target, String)>,
    pub rejected: Vec<Rejected>,
    /// Ids the response simply did not carry. The prompt invites omission, so
    /// this is an ordinary outcome and not an error.
    pub missing: Vec<String>,
}

/// One request's worth of work, owned so it can cross a task boundary.
pub(super) struct BatchJob {
    /// Carried, not re-checked: the token is proof the permission was on when
    /// the run started, and `provider::complete` will not compile without one.
    pub(super) consent: AiConsent,
    pub(super) provider: AiProvider,
    pub(super) api_key: Option<String>,
    pub(super) local_port: u16,
    pub(super) model: String,
    pub(super) lang: String,
    pub(super) role: UiRole,
    pub(super) units: Vec<BatchUnit>,
    pub(super) terms: Vec<(String, String)>,
    /// The run's cancel flag. A batch that is already in flight decides its
    /// own retries, so without this the dispatch loop's check is invisible
    /// here and a cancelled run can still issue hundreds of paid requests.
    pub(super) cancel: Arc<AtomicBool>,
}

pub(super) struct Written {
    pub(super) namespace: String,
    pub(super) key: String,
    pub(super) value: String,
    pub(super) source_en: String,
}

pub(super) struct BatchOutput {
    pub(super) role: UiRole,
    /// English → accepted translation, one entry per accepted UNIT. Feeds the
    /// cache and, after a name pass, the glossary fold.
    pub(super) answers: BTreeMap<String, String>,
    /// One entry per KEY: the fan-out `decide_batch` performed.
    pub(super) writes: Vec<Written>,
    pub(super) rejected: u32,
    /// Set when the model's answer could not be parsed at all, so this batch
    /// wrote nothing. It does NOT fail the run: the next batch is a different
    /// prompt over different strings and has every chance of parsing. Counted
    /// on the summary so the loss is reported rather than swallowed.
    pub(super) unusable: bool,
    /// Units this batch accounted for, however it accounted for them — the
    /// progress bar's denominator is units, not outcomes.
    pub(super) resolved: u32,
    pub(super) prompt_tokens: u32,
    pub(super) completion_tokens: u32,
    pub(super) usage_known: bool,
}

impl BatchOutput {
    pub(super) fn new(role: UiRole, resolved: usize) -> Self {
        Self {
            role,
            answers: BTreeMap::new(),
            writes: Vec::new(),
            rejected: 0,
            unusable: false,
            resolved: clamp_u32(resolved),
            prompt_tokens: 0,
            completion_tokens: 0,
            usage_known: true,
        }
    }

    fn note_usage(&mut self, usage: Option<provider::Usage>) {
        match usage {
            Some(u) => {
                self.prompt_tokens = self.prompt_tokens.saturating_add(u.prompt_tokens);
                self.completion_tokens = self.completion_tokens.saturating_add(u.completion_tokens);
            }
            None => self.usage_known = false,
        }
    }

    /// Record one accepted answer for one key.
    fn write_target(&mut self, target: &Target, value: &str, source_en: &str) {
        self.writes.push(Written {
            namespace: target.namespace.clone(),
            key: target.key.clone(),
            value: value.to_string(),
            source_en: source_en.to_string(),
        });
        self.answers
            .insert(source_en.to_string(), value.to_string());
    }

    /// Record one accepted answer across every key waiting on it — the
    /// single-string retry path, which starts from the unit rather than from
    /// [`decide_batch`]'s already-fanned-out list.
    fn accept(&mut self, unit: &BatchUnit, value: &str) {
        for target in &unit.targets {
            self.write_target(target, value, &unit.source_en);
        }
    }
}

/// Translate one batch, then give each refused string exactly one more chance.
pub(super) async fn translate_batch(job: BatchJob) -> Result<BatchOutput> {
    // A batch can be dispatched and then cancelled before its request leaves.
    // Cancel means "stop spending", and a batch that never asks costs exactly
    // its keys' English showing through — which the resource-pack merge makes
    // free.
    if is_cancelled(&job.cancel) {
        return Ok(BatchOutput::new(job.role, 0));
    }

    let terms = borrowed(&job.terms);
    let pairs: Vec<(String, String)> = job
        .units
        .iter()
        .map(|u| (u.id.clone(), u.source_en.clone()))
        .collect();
    let user = prompt::build_user_prompt(&job.lang, job.role, &pairs, &terms);

    let completion = complete_with_retry(&job, &user).await?;
    let answers = match prompt::parse_response(&completion.content) {
        Ok(answers) => answers,
        Err(reason) => {
            // Losing the batch, not the run. A malformed answer is a fact
            // about THIS reply; the next batch is a different prompt over
            // different strings. Aborting everything is reserved for a
            // provider that cannot serve us at all — an unretryable non-2xx,
            // which `complete_with_retry` already surfaces above.
            //
            // The raw answer is logged because it is the only thing that can
            // later distinguish a decoy object in the preamble from a model
            // that genuinely replied in prose. It is diagnostics, never the
            // user-facing error: it can be long, and it is not their problem.
            crate::diag!(
                "[l10n] prefill: unusable answer ({reason}): {}",
                completion
                    .content
                    .chars()
                    .take(RAW_ANSWER_LOG_CAP)
                    .collect::<String>()
            );
            let mut out = BatchOutput::new(job.role, job.units.len());
            out.unusable = true;
            // The tokens were spent whether or not the answer was usable.
            out.note_usage(completion.usage);
            return Ok(out);
        }
    };

    let outcome = decide_batch(&job.units, &answers);
    let mut out = BatchOutput::new(job.role, job.units.len());
    out.note_usage(completion.usage);

    // `decide_batch` fanned every accepted answer out across its targets but,
    // being pure, does not carry the English each was translated against — and
    // the store records exactly that as its staleness oracle. One index over
    // the units puts it back.
    let sources: BTreeMap<(&str, &str), &str> = job
        .units
        .iter()
        .flat_map(|unit| {
            unit.targets.iter().map(move |target| {
                (
                    (target.namespace.as_str(), target.key.as_str()),
                    unit.source_en.as_str(),
                )
            })
        })
        .collect();
    for (target, value) in &outcome.accepted {
        let Some(source_en) = sources.get(&(target.namespace.as_str(), target.key.as_str())) else {
            continue;
        };
        out.write_target(target, value, source_en);
    }

    retry_rejections(&job, &outcome, &terms, &mut out).await;
    Ok(out)
}

/// Exactly one single-string retry per rejected unit, with the verifier's own
/// complaint fed back. A second refusal writes nothing at all, and the mod's
/// English survives untouched — which is why refusing is cheap.
///
/// Split out of [`translate_batch`] so the cancel check is testable: this loop
/// is where a cancelled run could otherwise still spend the most. Ten in-flight
/// batches can each queue up to `MAX_BATCH` fresh single-string requests here,
/// every one of them decided AFTER the user pressed Cancel, and every one of
/// them free to retry three times at a 180 s timeout.
async fn retry_rejections(
    job: &BatchJob,
    outcome: &BatchOutcome,
    terms: &[(&str, &str)],
    out: &mut BatchOutput,
) {
    for rejection in &outcome.rejected {
        if is_cancelled(&job.cancel) {
            break;
        }
        let Some(unit) = job.units.iter().find(|u| u.id == rejection.id) else {
            continue;
        };
        match retry_one(job, unit, terms, &rejection.reason, out).await {
            Some(value) => out.accept(unit, &value),
            None => out.rejected += 1,
        }
    }
}

/// One retry for one string. Never fails the run: the batch around it already
/// succeeded and was paid for, so a retry that cannot be sent, cannot be
/// parsed, or is refused again all mean the same thing — this one string stays
/// English.
async fn retry_one(
    job: &BatchJob,
    unit: &BatchUnit,
    terms: &[(&str, &str)],
    reason: &RejectReason,
    out: &mut BatchOutput,
) -> Option<String> {
    let pairs = vec![(unit.id.clone(), unit.source_en.clone())];
    let mut user = prompt::build_user_prompt(&job.lang, job.role, &pairs, terms);
    user.push_str(&retry_note(&unit.id, reason));

    let completion = match complete_with_retry(job, &user).await {
        Ok(c) => c,
        Err(e) => {
            crate::diag!(
                "[l10n] prefill: retry for {} could not be sent: {}",
                unit.id,
                log_safe(&e)
            );
            return None;
        }
    };
    out.note_usage(completion.usage);

    let answers = prompt::parse_response(&completion.content).unwrap_or_default();
    let answer = answers.get(&unit.id)?;
    verify::verify(&unit.source_en, answer)
        .is_ok()
        .then(|| answer.clone())
}

/// The complaint appended to a retry prompt. Says what was wrong in the terms
/// the verifier actually checks, so the second attempt has something to act on
/// rather than "try again".
fn retry_note(id: &str, reason: &RejectReason) -> String {
    let complaint = match reason {
        RejectReason::Empty => "it was empty".to_string(),
        RejectReason::Format { error } => format_complaint(error),
        RejectReason::PlaceholdersChanged => {
            "it did not carry exactly the same %s / %1$s placeholders as the English".to_string()
        }
        RejectReason::ColourCodesChanged => {
            "it did not carry the same § colour codes, in the same order, as the English"
                .to_string()
        }
        RejectReason::ControlCharacter => {
            "it contained a line break or control character the English did not".to_string()
        }
        RejectReason::LiteralMissing { literal } => {
            format!("it dropped `{literal}`, which must be copied through unchanged")
        }
    };
    format!(
        "\n\nYour previous answer for \"{id}\" was rejected because {complaint}. \
         Answer again for \"{id}\" only, fixing exactly that. If you cannot, omit \
         the id."
    )
}

/// The grammar half of a complaint, spelled out rather than `Debug`-printed.
/// `FormatError` has no `Display` — it is an IPC type the UI localises — and
/// `UnsupportedSpecifier { specifier: 'd' }` in a prompt teaches a model
/// nothing about what to write instead.
fn format_complaint(error: &crate::l10n::validate::FormatError) -> String {
    use crate::l10n::validate::FormatError;
    match error {
        FormatError::UnsupportedSpecifier { specifier } => format!(
            "it used %{specifier}, which Minecraft does not support — only %s and %1$s are valid"
        ),
        FormatError::DanglingPercent => {
            "it ended with a bare % that begins no placeholder".to_string()
        }
        FormatError::IndexOutOfRange { index, available } => format!(
            "it referenced %{index}$s, but the English string supplies only {available} argument(s)"
        ),
    }
}

fn borrowed(terms: &[(String, String)]) -> Vec<(&str, &str)> {
    terms
        .iter()
        .map(|(en, tr)| (en.as_str(), tr.as_str()))
        .collect()
}

/// Decide what one response is worth, per requested string. Pure: it is the
/// whole judgement of the run, and it must be testable without a provider.
///
/// An id the response omits is `missing`, not an error — the prompt explicitly
/// invites omission, so the contract with a response is CONTAINMENT, never
/// key-set equality. An id that was never requested is dropped silently: a
/// hallucinated id must not become an override for a key nobody asked about.
#[must_use]
pub fn decide_batch(units: &[BatchUnit], answers: &BTreeMap<String, String>) -> BatchOutcome {
    let mut out = BatchOutcome::default();
    for unit in units {
        let Some(answer) = answers.get(&unit.id) else {
            out.missing.push(unit.id.clone());
            continue;
        };
        match verify::verify(&unit.source_en, answer) {
            Ok(()) => out.accepted.extend(
                unit.targets
                    .iter()
                    .map(|target| (target.clone(), answer.clone())),
            ),
            Err(reason) => out.rejected.push(Rejected {
                id: unit.id.clone(),
                reason,
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units() -> Vec<BatchUnit> {
        vec![
            BatchUnit {
                id: "s0".into(),
                source_en: "Copper Ingot".into(),
                targets: vec![Target {
                    namespace: "create".into(),
                    key: "item.create.a".into(),
                }],
            },
            BatchUnit {
                id: "s1".into(),
                source_en: "Uses %s of %s".into(),
                targets: vec![Target {
                    namespace: "create".into(),
                    key: "item.create.b".into(),
                }],
            },
        ]
    }

    fn answers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn accepts_answers_that_preserve_placeholders() {
        let out = decide_batch(
            &units(),
            &answers(&[("s0", "Медный слиток"), ("s1", "Тратит %s из %s")]),
        );
        assert_eq!(out.accepted.len(), 2);
        assert!(out.rejected.is_empty());
        assert!(out.missing.is_empty());
    }

    #[test]
    fn rejects_an_answer_that_drops_a_placeholder() {
        let out = decide_batch(&units(), &answers(&[("s1", "Тратит ресурсы")]));
        assert!(out.accepted.is_empty());
        assert_eq!(out.rejected.len(), 1);
        assert_eq!(out.rejected[0].id, "s1");
    }

    #[test]
    fn ignores_an_id_that_was_never_requested() {
        // A hallucinated id must not become an override for something the
        // user never asked about.
        let out = decide_batch(&units(), &answers(&[("s99", "Что-то")]));
        assert!(out.accepted.is_empty());
        assert_eq!(out.missing.len(), 2);
    }

    #[test]
    fn an_omitted_id_is_simply_missing_not_an_error() {
        // The prompt invites omission, so key-set EQUALITY is not the
        // contract — containment is.
        let out = decide_batch(&units(), &answers(&[("s0", "Медный слиток")]));
        assert_eq!(out.accepted.len(), 1);
        assert_eq!(out.missing, vec!["s1".to_string()]);
    }

    #[test]
    fn an_empty_or_whitespace_answer_is_rejected() {
        let out = decide_batch(&units(), &answers(&[("s0", "   ")]));
        assert!(out.accepted.is_empty());
        assert_eq!(out.rejected.len(), 1);
    }

    #[test]
    fn one_accepted_answer_fans_out_to_every_target() {
        let unit = BatchUnit {
            id: "s0".into(),
            source_en: "Energy".into(),
            targets: vec![
                Target {
                    namespace: "create".into(),
                    key: "item.create.a".into(),
                },
                Target {
                    namespace: "ae2".into(),
                    key: "item.ae2.b".into(),
                },
            ],
        };
        let out = decide_batch(&[unit], &answers(&[("s0", "Энергия")]));
        assert_eq!(out.accepted.len(), 2, "dedup must fan back out");
    }

    // -----------------------------------------------------------------
    // retry
    // -----------------------------------------------------------------

    #[test]
    fn the_retry_note_names_the_string_and_quotes_the_verifier() {
        let note = retry_note("s3", &RejectReason::PlaceholdersChanged);
        assert!(note.contains("s3"));
        assert!(
            note.contains("%s"),
            "a retry that does not say what was wrong is just 'try again': {note}"
        );

        let note = retry_note(
            "s4",
            &RejectReason::LiteralMissing {
                literal: "minecraft:diamond".to_string(),
            },
        );
        assert!(note.contains("minecraft:diamond"));
    }

    // -----------------------------------------------------------------
    // usage accounting
    // -----------------------------------------------------------------

    #[test]
    fn usage_is_unknown_as_soon_as_one_completion_omits_it() {
        let mut out = BatchOutput::new(UiRole::Name, 2);
        assert!(out.usage_known, "no calls yet means nothing was spent");
        out.note_usage(Some(provider::Usage {
            prompt_tokens: 10,
            completion_tokens: 4,
        }));
        assert!(out.usage_known);
        assert_eq!(out.prompt_tokens, 10);
        // A local server reports none. "Free" and "we do not know" are
        // different claims and the total must not silently become the first.
        out.note_usage(None);
        assert!(!out.usage_known);
        assert_eq!(out.prompt_tokens, 10);
    }

    // -----------------------------------------------------------------
    // cancellation, and what a failed run still owes the user
    // -----------------------------------------------------------------

    fn cancellable_job(cancel: Arc<AtomicBool>) -> BatchJob {
        BatchJob {
            consent: AiConsent::for_test(),
            // Port 1 is never a model server. If a cancel check is removed,
            // these tests start making a real (immediately refused) loopback
            // request instead of returning early — which is exactly what the
            // assertions below detect.
            provider: AiProvider::Local,
            api_key: None,
            local_port: 1,
            model: "test-model".to_string(),
            lang: "ru_ru".to_string(),
            role: UiRole::Name,
            units: units(),
            terms: Vec::new(),
            cancel,
        }
    }

    #[tokio::test]
    async fn a_cancelled_run_issues_no_further_single_string_retries() {
        // Ten in-flight batches can each queue up to MAX_BATCH fresh retries
        // here — up to 400 new paid requests decided AFTER the user pressed
        // Cancel, each free to try three times at a 180 s timeout. Skipping a
        // retry is free by this module's own argument: the key is simply not
        // written and the mod's English shows.
        let job = cancellable_job(Arc::new(AtomicBool::new(true)));
        let outcome = BatchOutcome {
            accepted: Vec::new(),
            rejected: vec![Rejected {
                id: "s0".to_string(),
                reason: RejectReason::Empty,
            }],
            missing: Vec::new(),
        };
        let mut out = BatchOutput::new(UiRole::Name, 1);

        retry_rejections(&job, &outcome, &[], &mut out).await;

        assert!(out.writes.is_empty());
        assert_eq!(
            out.rejected, 0,
            "a cancelled run must not spend on a retry it decided after the cancel"
        );
    }

    #[tokio::test]
    async fn a_batch_cancelled_before_it_is_sent_spends_nothing() {
        let job = cancellable_job(Arc::new(AtomicBool::new(true)));
        let out = translate_batch(job)
            .await
            .expect("a cancelled batch is not a failure");
        assert!(out.writes.is_empty());
        assert_eq!(out.resolved, 0);
    }
}

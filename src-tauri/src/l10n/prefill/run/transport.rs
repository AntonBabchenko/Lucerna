//! Getting one completion out of a provider: what is worth retrying, how long
//! it waits, when it gives up because the run was cancelled — and what may be
//! said about a failure in a file the user can publish.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::l10n::prefill::prompt;
use crate::l10n::prefill::provider;

use super::batch::BatchJob;

/// Total attempts per request: the first plus two retries, and only for
/// failures that can plausibly differ next time (see [`is_retryable`]).
const TRANSPORT_ATTEMPTS: u32 = 3;

/// First retry wait, doubled per attempt. Deliberately not
/// `network::request::backoff_after_429` — that one is private, needs the
/// response headers this layer never sees, and is already applied inside the
/// chokepoint, which retries a 429 three times before we are told anything.
const RETRY_BACKOFF: Duration = Duration::from_secs(1);

/// What may be said about an [`Error`] in the launcher log.
///
/// `Error::L10nPrefillProvider`'s `Display` interpolates `details` — the
/// provider's own response body. That body routinely echoes the request that
/// caused it, and the request carried the user's API key in an `Authorization:
/// Bearer` header. `crate::diag!` appends to `lucerna.log`, which the Logs
/// viewer uploads verbatim to the public paste service mclo.gs, and
/// `l10n::logs::share`'s anonymiser has no rule for `Bearer`, `sk-`,
/// `x-api-key` or any other provider key shape. So a provider error may reach
/// a dialog in full — ephemeral, and the user's own screen — but only its
/// discriminating fields ever reach a file that can be published.
///
/// Every error formatted into a log line in this module goes through here.
pub(super) fn log_safe(error: &Error) -> String {
    match error {
        Error::L10nPrefillProvider {
            provider, status, ..
        } => format!("provider {provider}, HTTP {status}"),
        // The URL is ours (an allowlisted endpoint or 127.0.0.1); the reqwest
        // detail string is not worth the risk of finding out what it quotes.
        Error::Network { url, .. } => format!("network failure for {url}"),
        other => other.to_string(),
    }
}

/// Send one completion, retrying only what could plausibly differ next time.
pub(super) async fn complete_with_retry(
    job: &BatchJob,
    user: &str,
) -> Result<provider::Completion> {
    let mut attempt = 1u32;
    loop {
        let error = match provider::complete(
            &job.consent,
            job.provider,
            job.api_key.as_deref(),
            job.local_port,
            &job.model,
            prompt::system_prompt(),
            user,
        )
        .await
        {
            Ok(completion) => return Ok(completion),
            Err(e) => e,
        };

        if attempt >= TRANSPORT_ATTEMPTS || !is_retryable(&error) || is_cancelled(&job.cancel) {
            return Err(error);
        }
        crate::diag!(
            "[l10n] prefill: attempt {attempt} failed ({}), retrying",
            log_safe(&error)
        );
        // A cancel during the wait has to be seen now, not after the full
        // backoff: the point of cancelling is to stop spending, and the next
        // thing after this sleep is another paid request.
        if !sleep_unless_cancelled(&job.cancel, RETRY_BACKOFF * 2u32.pow(attempt - 1)).await {
            return Err(error);
        }
        attempt += 1;
    }
}

/// Whether the run has been cancelled. `SeqCst` to match `prefill::cancel`,
/// which is written by a separate sync IPC command.
pub(super) fn is_cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::SeqCst)
}

/// Sleep for `total`, giving up as soon as `cancel` is set. Returns `false`
/// when it was cut short.
///
/// Polled in slices rather than woken by a `Notify` because the cancel
/// registry is a plain `AtomicBool` shared with a synchronous IPC command;
/// adding a notifier beside it would be a second source of truth for the same
/// fact.
async fn sleep_unless_cancelled(cancel: &AtomicBool, total: Duration) -> bool {
    const SLICE: Duration = Duration::from_millis(100);
    let mut left = total;
    while !left.is_zero() {
        if is_cancelled(cancel) {
            return false;
        }
        let step = left.min(SLICE);
        tokio::time::sleep(step).await;
        left -= step;
    }
    !is_cancelled(cancel)
}

/// Whether sending the identical request again could plausibly succeed.
///
/// A 401 or a 400 will not, and retrying one only delays telling the user what
/// is wrong. `status: 0` means the body did not parse — deterministic for the
/// same request, so it is not retried here either; the single-string retry
/// above is the answer to a model that answers badly.
fn is_retryable(error: &Error) -> bool {
    match error {
        Error::Network { .. } => true,
        Error::L10nPrefillProvider { status, .. } => *status == 429 || *status >= 500,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // -----------------------------------------------------------------
    // retry
    // -----------------------------------------------------------------

    #[test]
    fn only_failures_that_could_differ_next_time_are_retried() {
        assert!(is_retryable(&Error::Network {
            url: "https://example.com".into(),
            details: "reset".into()
        }));
        assert!(is_retryable(&provider_error(503)));
        assert!(is_retryable(&provider_error(429)));
        // An auth failure and an unparseable body are deterministic: retrying
        // only delays telling the user what is actually wrong.
        assert!(!is_retryable(&provider_error(401)));
        assert!(!is_retryable(&provider_error(0)));
    }

    fn provider_error(status: u16) -> Error {
        Error::L10nPrefillProvider {
            provider: "groq".into(),
            status,
            details: String::new(),
        }
    }

    // -----------------------------------------------------------------
    // what may be written to a publishable log
    // -----------------------------------------------------------------

    #[test]
    fn a_provider_error_body_never_reaches_the_launcher_log() {
        // `crate::diag!` appends to lucerna.log, the Logs viewer uploads that
        // file verbatim to the public paste service mclo.gs, and the share
        // anonymiser has no rule for `Bearer`, `sk-` or any provider key
        // shape. A provider's error body echoes the request that caused it,
        // and that request carried the user's key.
        let err = Error::L10nPrefillProvider {
            provider: "groq".to_string(),
            status: 401,
            details: "invalid_api_key for Bearer sk-live-9f3ac0b1d2e4".to_string(),
        };

        let line = log_safe(&err);
        assert!(
            !line.contains("sk-live-9f3ac0b1d2e4"),
            "the user's key reached a publishable log: {line}"
        );
        assert!(!line.contains("Bearer"), "{line}");
        assert!(
            line.contains("groq") && line.contains("401"),
            "a laundered line must still say which provider failed and how: {line}"
        );

        // And the reason this function has to exist: the Display impl carries
        // the whole body, which is right for a dialog and wrong for a file.
        assert!(err.to_string().contains("sk-live-9f3ac0b1d2e4"));
    }

    #[test]
    fn a_network_failure_is_logged_by_destination_only() {
        let err = Error::Network {
            url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
            details: "the transport said something we did not audit".to_string(),
        };
        let line = log_safe(&err);
        assert!(line.contains("api.groq.com"), "{line}");
        assert!(!line.contains("did not audit"), "{line}");
    }

    /// Every source file of the `run` module, paired with its file name.
    ///
    /// `include_str!` embeds each at compile time, exactly as the single-file
    /// version of this scan did. The list is hand-written, so the scan below
    /// starts by proving it still names every file in the directory.
    const SOURCES: &[(&str, &str)] = &[
        ("mod.rs", include_str!("mod.rs")),
        ("batch.rs", include_str!("batch.rs")),
        ("fixtures.rs", include_str!("fixtures.rs")),
        ("pipeline.rs", include_str!("pipeline.rs")),
        ("state.rs", include_str!("state.rs")),
        ("transport.rs", include_str!("transport.rs")),
        ("types.rs", include_str!("types.rs")),
    ];

    #[test]
    fn no_diagnostic_line_in_this_module_formats_an_error_directly() {
        // The leak is in the FORMATTING, not the control flow: a future
        // `("… ({e})")` would compile, pass every behavioural test, and
        // quietly append a provider body to a file the share button
        // publishes. Reading this module's own source is the only thing that
        // catches the next one. The needle is assembled at runtime so the
        // scan cannot match itself.
        //
        // "This module" is a directory, so the scan is only worth its name if
        // it covers all of it: a file added beside these would otherwise be
        // exempt from the one rule this module has.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/l10n/prefill/run");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .expect("the module's own source directory")
            .map(|entry| {
                entry
                    .expect("a readable directory entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name.ends_with(".rs"))
            .collect();
        on_disk.sort();
        let mut scanned: Vec<String> = SOURCES
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect();
        scanned.sort();
        assert_eq!(
            scanned, on_disk,
            "a source file of this module is not covered by the diagnostic scan"
        );

        let needle = concat!("diag", "!(");
        let open = needle.len() - 1;

        for (file, src) in SOURCES {
            let mut rest = *src;
            while let Some(at) = rest.find(needle) {
                let tail = &rest[at..];
                let mut depth: i32 = 0;
                let mut end = tail.len();
                for (i, c) in tail[open..].char_indices() {
                    if c == '(' {
                        depth += 1;
                    } else if c == ')' {
                        depth -= 1;
                        if depth <= 0 {
                            end = open + i + 1;
                            break;
                        }
                    }
                }
                let call = &tail[..end];
                assert!(
                    !call.contains("{e}") || call.contains("log_safe"),
                    "a diagnostic line in {file} formats `e` without laundering it: {call}"
                );
                rest = &tail[end..];
            }
        }
    }

    // -----------------------------------------------------------------
    // cancellation
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn a_backoff_wait_gives_up_the_moment_the_run_is_cancelled() {
        let cancel = Arc::new(AtomicBool::new(false));
        let flipper = Arc::clone(&cancel);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            flipper.store(true, Ordering::SeqCst);
        });

        let started = std::time::Instant::now();
        let slept_through = sleep_unless_cancelled(&cancel, Duration::from_secs(30)).await;
        assert!(!slept_through);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "a cancelled wait must not serve out its backoff before noticing"
        );
    }
}

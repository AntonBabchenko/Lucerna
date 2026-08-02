//! One OpenAI-compatible chat-completion call, per provider.
//!
//! All four backends speak the same `/chat/completions` shape, so there is one
//! request builder and one response parser; only the URL, the auth header and
//! the default model differ. Cloud providers go through the ordinary
//! `network::request` chokepoint (their hosts are on the allowlist); `Local`
//! goes through `network::loopback`, which owns the 127.0.0.1 constant.
//!
//! Consent is checked in `prefill::run` before anything here is reachable —
//! `structural_prefill_consent_order.rs` pins that ordering.
//!
//! Two documented quirks of Anthropic's OpenAI-compatibility layer shape this
//! module: it ignores `response_format` (so no JSON mode is requested — the
//! prompt module parses leniently instead) and it caps `temperature` at 1.

use crate::error::{Error, Result};
use crate::instances::schema::AiProvider;
use serde::Deserialize;
use std::time::Duration;

/// Total budget for one completion, connect through body. Generous on purpose:
/// a batch of forty strings on a local CPU model can legitimately take minutes,
/// and the generation client has no read timeout to cut it short.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

/// Full chat endpoints, verified against each vendor's own documentation on
/// 2026-08-02. The path differs per vendor (Gemini's compatibility layer lives
/// under `/v1beta/openai`), so these are whole URLs rather than a shared path.
const ANTHROPIC_ENDPOINT: &str = "https://api.anthropic.com/v1/chat/completions";
const GEMINI_ENDPOINT: &str =
    "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions";
const GROQ_ENDPOINT: &str = "https://api.groq.com/openai/v1/chat/completions";

/// Path component for the local server. The host is NOT ours to choose —
/// [`complete`] passes this path to `network::loopback::post_json`, which
/// supplies 127.0.0.1 itself.
const LOCAL_CHAT_PATH: &str = "/v1/chat/completions";

/// Low, not zero: translation wants near-determinism, and a hard 0 makes some
/// backends degenerate into repetition. Anthropic's layer caps this at 1.
const TEMPERATURE: f64 = 0.2;

/// How much of a provider's response may be quoted in an error. A provider
/// error body routinely echoes the request that caused it, and the user may
/// paste the error into a bug report — so the quote is capped, and the body
/// goes nowhere else (no log line, no second error field).
const DETAILS_CAP: usize = 200;

/// Chokepoint attribution for the launcher log.
const INITIATOR: &str = "l10n-prefill";

/// The smallest prompt that still exercises credential, endpoint and model
/// name. Used only by [`test_credentials`], which ignores the answer.
const PROBE_SYSTEM: &str = "Reply with the single word OK.";
const PROBE_USER: &str = "OK";

impl AiProvider {
    /// Stable id. This is the keychain account name and the IPC argument —
    /// renaming one orphans every key a user has already stored.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            AiProvider::Anthropic => "anthropic",
            AiProvider::Gemini => "gemini",
            AiProvider::Groq => "groq",
            AiProvider::Local => "local",
        }
    }

    /// The chat-completions URL. `local_port` is consulted only by `Local`;
    /// the cloud endpoints are fixed constants.
    #[must_use]
    pub fn endpoint(self, local_port: u16) -> String {
        match self {
            AiProvider::Anthropic => ANTHROPIC_ENDPOINT.to_string(),
            AiProvider::Gemini => GEMINI_ENDPOINT.to_string(),
            AiProvider::Groq => GROQ_ENDPOINT.to_string(),
            AiProvider::Local => format!("http://127.0.0.1:{local_port}{LOCAL_CHAT_PATH}"),
        }
    }

    /// Whether this backend needs an API key. Written as an exhaustive match
    /// rather than a negated `matches!` so a new variant is a compile error
    /// here instead of a silent "no key needed".
    #[must_use]
    pub fn needs_key(self) -> bool {
        match self {
            AiProvider::Anthropic | AiProvider::Gemini | AiProvider::Groq => true,
            AiProvider::Local => false,
        }
    }

    /// Model used when the user left the field blank. `Local` has none —
    /// nothing can enumerate a local server's models offline, so the user
    /// supplies the name.
    #[must_use]
    pub fn default_model(self) -> &'static str {
        match self {
            AiProvider::Anthropic => "claude-haiku-4-5-20251001",
            AiProvider::Gemini => "gemini-3.6-flash",
            AiProvider::Groq => "llama-3.3-70b-versatile",
            AiProvider::Local => "",
        }
    }
}

/// Token counts as reported by the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Usage {
    /// A provider that reports only one of the two still parses.
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
}

/// One assistant answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub content: String,
    /// `None` when the provider reported no usage at all — a local server
    /// typically does not. A zeroed [`Usage`] would read as "this was free",
    /// which is a different claim from "we do not know what this cost".
    pub usage: Option<Usage>,
}

#[derive(Deserialize)]
struct RawCompletion {
    #[serde(default)]
    choices: Vec<RawChoice>,
    /// Deliberately a `Value` rather than `Option<Usage>`: usage is
    /// bookkeeping, and a provider reporting it in an unexpected shape must
    /// not cost us a translation we have already paid for.
    #[serde(default)]
    usage: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawChoice {
    #[serde(default)]
    message: Option<RawMessage>,
}

#[derive(Deserialize)]
struct RawMessage {
    #[serde(default)]
    content: Option<String>,
}

/// The request body every backend understands. `response_format` is
/// deliberately absent — see the module docs.
#[must_use]
pub fn build_request_body(model: &str, system: &str, user: &str) -> Vec<u8> {
    let payload = serde_json::json!({
        "model": model,
        "temperature": TEMPERATURE,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
    });
    // Unreachable: `payload` is a `Value` built from `&str`s and one finite
    // f64. Serialising a `Value` can only fail on a non-string map key or a
    // non-finite number, neither of which this literal can contain.
    serde_json::to_vec(&payload).expect("chat request body serialises")
}

/// Lift the assistant message out of an OpenAI-shaped answer.
///
/// `Err` carries a short reason rather than a typed error because this
/// function does not know which provider answered; [`complete`] folds the
/// reason into `Error::L10nPrefillProvider` with `status: 0`.
pub fn parse_completion(body: &[u8]) -> std::result::Result<Completion, String> {
    let raw: RawCompletion =
        serde_json::from_slice(body).map_err(|e| format!("malformed JSON: {e}"))?;
    // First choice that actually carries content — an empty `choices` array,
    // or choices with no message, is an error rather than an empty string: an
    // empty translation would be written over the mod's own English.
    let content = raw
        .choices
        .into_iter()
        .find_map(|c| c.message.and_then(|m| m.content))
        .ok_or_else(|| "response carried no assistant message".to_string())?;
    let usage = raw
        .usage
        .and_then(|v| serde_json::from_value::<Usage>(v).ok());
    Ok(Completion { content, usage })
}

/// Build the typed provider error, with the quoted body capped. See
/// [`DETAILS_CAP`] — this is the only place a provider body is quoted.
fn provider_error(provider: AiProvider, status: u16, details: &str) -> Error {
    Error::L10nPrefillProvider {
        provider: provider.id().to_string(),
        status,
        details: details.chars().take(DETAILS_CAP).collect(),
    }
}

/// POST one chat request and return the 2xx body. A received non-2xx becomes
/// [`Error::L10nPrefillProvider`]; a transport failure propagates from the
/// chokepoint unchanged.
async fn send_chat(
    provider: AiProvider,
    api_key: Option<&str>,
    local_port: u16,
    body: &[u8],
) -> Result<Vec<u8>> {
    let resp = match provider {
        AiProvider::Local => {
            crate::network::loopback::post_json(local_port, LOCAL_CHAT_PATH, body, REQUEST_TIMEOUT)
                .await?
        }
        AiProvider::Anthropic | AiProvider::Gemini | AiProvider::Groq => {
            let key = api_key
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .ok_or_else(|| Error::L10nPrefillKeyMissing {
                    provider: provider.id().to_string(),
                })?;
            // The one place the key is used. It goes into a header and nowhere
            // else — never a log line, never an error, never the URL.
            let authorization = format!("Bearer {key}");
            crate::network::request::post_with_timeout(
                &provider.endpoint(local_port),
                &[
                    ("authorization", authorization.as_str()),
                    ("content-type", "application/json"),
                ],
                body,
                INITIATOR,
                REQUEST_TIMEOUT,
            )
            .await?
        }
    };
    if !(200..300).contains(&resp.status) {
        // Cap the BYTES before the lossy conversion: a misconfigured local
        // server can answer with a multi-megabyte HTML error page and only
        // `DETAILS_CAP` characters survive anyway. Four is the longest UTF-8
        // encoding, so this head can never starve the character cap.
        let head = &resp.body[..resp.body.len().min(DETAILS_CAP * 4)];
        return Err(provider_error(
            provider,
            resp.status,
            &String::from_utf8_lossy(head),
        ));
    }
    Ok(resp.body)
}

/// Send one chat completion. `Err` means no usable answer; the caller decides
/// whether to retry, and never writes anything on failure.
pub async fn complete(
    provider: AiProvider,
    api_key: Option<&str>,
    local_port: u16,
    model: &str,
    system: &str,
    user: &str,
) -> Result<Completion> {
    let body = build_request_body(model, system, user);
    let answer = send_chat(provider, api_key, local_port, &body).await?;
    // `status: 0` — the transport succeeded, the body did not make sense.
    parse_completion(&answer).map_err(|reason| provider_error(provider, 0, &reason))
}

/// "Test key": the cheapest round-trip that proves credential and endpoint.
///
/// The answer is discarded on purpose. What is being tested is auth, host and
/// model name, and every one of those failures arrives as a non-2xx status —
/// so a model that replies with something unparseable is still a pass.
pub async fn test_credentials(
    provider: AiProvider,
    api_key: Option<&str>,
    local_port: u16,
    model: &str,
) -> Result<()> {
    let body = build_request_body(model, PROBE_SYSTEM, PROBE_USER);
    send_chat(provider, api_key, local_port, &body).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::schema::AiProvider;

    #[test]
    fn every_cloud_endpoint_is_https_and_on_the_allowlist() {
        for p in [AiProvider::Anthropic, AiProvider::Gemini, AiProvider::Groq] {
            let url = p.endpoint(0);
            assert!(url.starts_with("https://"), "{p:?} must be https");
            let host = url
                .trim_start_matches("https://")
                .split('/')
                .next()
                .expect("host");
            assert!(
                crate::network::allowlist::is_host_allowed(host),
                "{p:?} endpoint host {host} is not on the allowlist"
            );
        }
    }

    #[test]
    fn the_local_provider_is_loopback_on_the_configured_port() {
        assert_eq!(
            AiProvider::Local.endpoint(1234),
            "http://127.0.0.1:1234/v1/chat/completions"
        );
    }

    #[test]
    fn only_the_local_provider_works_without_a_key() {
        assert!(!AiProvider::Local.needs_key());
        for p in [AiProvider::Anthropic, AiProvider::Gemini, AiProvider::Groq] {
            assert!(p.needs_key());
        }
    }

    #[test]
    fn provider_ids_are_stable_and_unique() {
        // These are keychain account names — changing one orphans a stored key.
        assert_eq!(AiProvider::Anthropic.id(), "anthropic");
        assert_eq!(AiProvider::Gemini.id(), "gemini");
        assert_eq!(AiProvider::Groq.id(), "groq");
        assert_eq!(AiProvider::Local.id(), "local");
        let ids: std::collections::BTreeSet<_> = AiProvider::ALL.iter().map(|p| p.id()).collect();
        assert_eq!(ids.len(), AiProvider::ALL.len());
    }

    #[test]
    fn a_request_body_carries_model_system_and_user_messages() {
        let body = build_request_body("m-1", "SYS", "USER");
        let v: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON");
        assert_eq!(v["model"], "m-1");
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"][0]["content"], "SYS");
        assert_eq!(v["messages"][1]["role"], "user");
        assert_eq!(v["messages"][1]["content"], "USER");
        let temp = v["temperature"].as_f64().expect("temperature set");
        assert!(
            (0.0..=1.0).contains(&temp),
            "Anthropic caps temperature at 1"
        );
    }

    #[test]
    fn the_assistant_message_is_lifted_out_of_an_openai_shaped_answer() {
        let raw = br#"{"choices":[{"message":{"role":"assistant","content":"HELLO"}}],
                       "usage":{"prompt_tokens":10,"completion_tokens":4}}"#;
        let parsed = parse_completion(raw).expect("well-formed completion");
        assert_eq!(parsed.content, "HELLO");
        assert_eq!(
            parsed.usage,
            Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 4
            })
        );
    }

    #[test]
    fn a_completion_with_no_choices_is_an_error_not_an_empty_string() {
        assert!(parse_completion(br#"{"choices":[]}"#).is_err());
    }

    #[test]
    fn a_quoted_provider_body_is_capped_so_it_cannot_carry_the_key_back() {
        // Not in the plan's list, added because this is the one safety
        // property of the module with no other test: a provider error body
        // routinely echoes the request that caused it, the request carried the
        // API key, and this error is shown to the user and pasted into bug
        // reports. Forgetting the cap is a silent, plausible mistake.
        let echoed = format!("{}sk-the-users-actual-key", "x".repeat(400));
        let err = provider_error(AiProvider::Groq, 401, &echoed);
        match err {
            Error::L10nPrefillProvider {
                provider,
                status,
                details,
            } => {
                assert_eq!(provider, "groq");
                assert_eq!(status, 401);
                assert_eq!(details.chars().count(), DETAILS_CAP);
                assert!(!details.contains("sk-the-users-actual-key"));
            }
            other => panic!("expected L10nPrefillProvider, got {other:?}"),
        }
    }

    #[test]
    fn usage_is_absent_rather_than_zero_when_the_server_omits_it() {
        // A local server reports no usage. Showing "cost: 0" would be a lie;
        // the UI must be able to tell "free" from "unknown".
        let parsed =
            parse_completion(br#"{"choices":[{"message":{"content":"X"}}]}"#).expect("valid");
        assert_eq!(parsed.usage, None);
    }
}

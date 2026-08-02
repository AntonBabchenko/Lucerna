//! The AI pre-fill pipeline, end to end, against a mock OpenAI-compatible
//! provider.
//!
//! Target: `l10n::prefill::run::execute`, not `run`. No integration test in
//! this repository can build a `tauri::AppHandle` (`download_integration.rs`,
//! `worlds_integration.rs` and `servers_diagnose.rs` all say so), and `run`
//! takes one for consent, settings, the keychain, paths and the pack rebuild.
//! `execute` is the half that works on plain data; `RunContext::for_settings`
//! is how a test hands it that data, consent included and consent-checked.
//!
//! **Every test here drives the `Local` provider.** That is not a shortcut: the
//! cloud endpoints are compile-time constants in `prefill::provider`
//! (`AiProvider::endpoint`), there is no seam that redirects them, and inventing
//! one would mean shipping a way to point a user's API key at an arbitrary host.
//! `Local` already targets `http://127.0.0.1:<port>/v1/chat/completions` through
//! `network::loopback`, which is exactly what a wiremock server is. The two
//! things that path does NOT exercise are the `Authorization: Bearer` header and
//! the chokepoint's own 429 retry — both covered by unit tests in
//! `prefill::provider` and `network::request`.
//!
//! No `test_seam::scope` here, deliberately: `network::loopback` bypasses the
//! host allowlist by construction — that is the whole reason it is a module
//! confined by `structural_loopback_confined.rs` — so there is no
//! `LUCERNA_EXTRA_ALLOWED_HOSTS` to install. The `test_lock` below serialises
//! the tests anyway, because the provider concurrency semaphore in
//! `prefill::run` is process-wide.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use serde_json::json;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use lucerna_lib::instances::schema::{AiProvider, GeneralSettings};
use lucerna_lib::l10n::prefill::glossary::Glossary;
use lucerna_lib::l10n::prefill::run::{execute, RunContext, RunSummary};
use lucerna_lib::l10n::store::{self, Entry, Origin};

/// The path `prefill::provider` POSTs to on a local server.
const CHAT_PATH: &str = "/v1/chat/completions";
const LANG: &str = "ru_ru";
const NAMESPACE: &str = "testmod";

/// Where `prompt::build_user_prompt` starts the JSON object of strings. Parsing
/// from here rather than from the first `{` keeps the mock honest about the
/// real prompt shape instead of guessing at it.
const STRINGS_MARKER: &str = "\nStrings:\n";

/// Serialise the tests: `prefill::run`'s provider slots are a process-wide
/// `Semaphore`, and the cancel test asserts on an exact request count.
fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// the world a run reads
// ---------------------------------------------------------------------------

/// An in-memory jar holding the given `(path, contents)` entries.
fn jar(entries: &[(String, String)]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        for (name, body) in entries {
            w.start_file(name.as_str(), opts).expect("start jar entry");
            w.write_all(body.as_bytes()).expect("write jar entry");
        }
        w.finish().expect("finish jar");
    }
    buf.into_inner()
}

struct World {
    /// Held so the temp tree outlives the run.
    _dir: TempDir,
    inst_root: PathBuf,
    store_dir: PathBuf,
}

impl World {
    /// Every override this run wrote, read back off disk.
    fn entries(&self) -> BTreeMap<String, Entry> {
        store::load(&self.store_dir, LANG, NAMESPACE).entries
    }
}

/// An instance with one enabled mod that ships English for `keys` and no
/// Russian at all — so every key is `KeyState::Missing` and worth one unit of
/// work. The override store starts empty, which is what makes that true.
fn world(keys: &[(&str, &str)]) -> World {
    let dir = tempfile::tempdir().expect("tempdir");
    let inst_root = dir.path().join("instance");
    let store_dir = dir.path().join("l10n");

    let en: BTreeMap<&str, &str> = keys.iter().copied().collect();
    let bytes = jar(&[(
        format!("assets/{NAMESPACE}/lang/en_us.json"),
        serde_json::to_string(&en).expect("lang map serialises"),
    )]);

    let mods_dir = lucerna_lib::mods::installed::mods_dir(&inst_root);
    std::fs::create_dir_all(&mods_dir).expect("mods dir");
    std::fs::write(mods_dir.join("testmod.jar"), bytes).expect("seed the jar");

    World {
        _dir: dir,
        inst_root,
        store_dir,
    }
}

/// A run context pointed at `port`. The consent token is minted by the real
/// gate from these settings — flipping `allow_ai_translation` to false here
/// makes `for_settings` refuse, which is the property that lets this seam
/// exist at all.
fn context(world: &World, port: u16) -> RunContext {
    let general = GeneralSettings {
        allow_ai_translation: true,
        ai_provider: AiProvider::Local,
        ai_model: "mock-model".to_string(),
        ai_local_port: port,
        ..GeneralSettings::default()
    };
    RunContext::for_settings(
        &general,
        None,
        world.inst_root.clone(),
        world.store_dir.clone(),
        LANG,
        None,
        Glossary::empty(),
    )
    .expect("the permission is on and the model is named")
}

/// Drive the pipeline. `execute` only returns `Err` when discovery itself
/// failed — a provider failure is reported ON the summary, so the tests below
/// read `summary.failed` rather than an `Err`.
async fn prefill(world: &World, server: &MockServer, cancel: &Arc<AtomicBool>) -> RunSummary {
    let port = server.address().port();
    execute(&context(world, port), cancel, &|_| {})
        .await
        .expect("discovery succeeds")
}

async fn prefill_uncancelled(world: &World, server: &MockServer) -> RunSummary {
    prefill(world, server, &Arc::new(AtomicBool::new(false))).await
}

async fn requests(server: &MockServer) -> Vec<Request> {
    server
        .received_requests()
        .await
        .expect("request recording is on by default")
}

// ---------------------------------------------------------------------------
// the mock provider
// ---------------------------------------------------------------------------

/// The `(id, English)` pairs one request asked about, lifted back out of the
/// user prompt at the exact place `prompt::build_user_prompt` writes them.
fn asked(req: &Request) -> BTreeMap<String, String> {
    let body: serde_json::Value = serde_json::from_slice(&req.body).expect("a JSON request body");
    let user = body["messages"][1]["content"]
        .as_str()
        .expect("a user message");
    let at = user
        .find(STRINGS_MARKER)
        .expect("the prompt must list the strings it wants translated");
    // Read ONE JSON value and stop. A retry prompt appends the verifier's
    // complaint after the object, so a whole-tail `from_str` rejects it as
    // trailing garbage — which the mock then answers with a dropped
    // connection, and the run charges to the transport retry budget.
    serde_json::Deserializer::from_str(&user[at + STRINGS_MARKER.len()..])
        .into_iter::<BTreeMap<String, String>>()
        .next()
        .expect("the prompt lists at least one string")
        .expect("the string list is a JSON object")
}

/// An OpenAI-shaped answer carrying `translations`. `usage` omitted is what a
/// local server really does, and what `usage_known` exists to distinguish from
/// a run that genuinely cost nothing.
fn answer(translations: &BTreeMap<String, String>, usage: bool) -> ResponseTemplate {
    let content = json!({ "translations": translations }).to_string();
    let mut body = json!({
        "choices": [{ "message": { "role": "assistant", "content": content } }]
    });
    if usage {
        body["usage"] = json!({ "prompt_tokens": 12, "completion_tokens": 7 });
    }
    ResponseTemplate::new(200).set_body_json(body)
}

/// A translation `prefill::verify` always accepts: the English is copied
/// through verbatim, so the placeholder multiset, the `§` sequence and every
/// do-not-translate literal survive by construction.
fn translated(source: &str) -> String {
    format!("ру {source}")
}

fn translate_all(req: &Request, usage: bool) -> ResponseTemplate {
    let out: BTreeMap<String, String> = asked(req)
        .into_iter()
        .map(|(id, en)| (id, translated(&en)))
        .collect();
    answer(&out, usage)
}

/// Mount a responder on the chat endpoint. Every test mounts exactly one, so
/// the request count on the server is the request count of the run.
async fn mount(
    server: &MockServer,
    responder: impl Fn(&Request) -> ResponseTemplate + Send + Sync + 'static,
) {
    Mock::given(method("POST"))
        .and(path(CHAT_PATH))
        .respond_with(responder)
        .mount(server)
        .await;
}

// ---------------------------------------------------------------------------
// 1. happy path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_finished_run_writes_every_verified_string_as_machine_origin() {
    let _g = test_lock();
    // One key per role, so all three passes run and the two-pass ordering is
    // exercised rather than assumed.
    let world = world(&[
        ("item.testmod.copper", "Copper Ingot"),
        ("testmod.tooltip.hold", "Hold %s to see more"),
        ("gui.testmod.confirm", "Confirm"),
    ]);
    let server = MockServer::start().await;
    mount(&server, |req: &Request| translate_all(req, true)).await;

    let summary = prefill_uncancelled(&world, &server).await;

    assert_eq!(summary.written, 3);
    assert_eq!(summary.rejected, 0);
    assert_eq!(summary.from_cache, 0);
    assert_eq!(summary.from_glossary, 0);
    assert!(!summary.cancelled);
    assert_eq!(summary.failed, None);
    assert!(summary.usage_known);
    // Three batches — Name, Prose, Other — at 12 + 7 reported tokens each.
    assert_eq!(requests(&server).await.len(), 3);
    assert_eq!(summary.prompt_tokens, 36);
    assert_eq!(summary.completion_tokens, 21);

    let entries = world.entries();
    assert_eq!(entries.len(), 3);
    for (key, source_en) in [
        ("item.testmod.copper", "Copper Ingot"),
        ("testmod.tooltip.hold", "Hold %s to see more"),
        ("gui.testmod.confirm", "Confirm"),
    ] {
        let entry = entries.get(key).unwrap_or_else(|| panic!("{key} written"));
        assert_eq!(entry.value, translated(source_en));
        assert_eq!(
            entry.origin,
            Origin::Machine,
            "a machine string must be reclaimable by a hand edit, which only \
             the origin distinguishes"
        );
        assert_eq!(
            entry.source_en, source_en,
            "the English it was translated against is the staleness oracle"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. a rate limit is waited out, once
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_rate_limited_batch_is_retried_exactly_once_and_then_succeeds() {
    let _g = test_lock();
    let world = world(&[("item.testmod.copper", "Copper Ingot")]);
    let server = MockServer::start().await;

    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    mount(&server, move |req: &Request| {
        if seen.fetch_add(1, Ordering::SeqCst) == 0 {
            // `retry-after` is decorative on this path: the chokepoint's
            // header-driven backoff is cloud-only, and the transport retry in
            // `prefill::run` uses its own fixed schedule.
            return ResponseTemplate::new(429).insert_header("retry-after", "1");
        }
        translate_all(req, true)
    })
    .await;

    let summary = prefill_uncancelled(&world, &server).await;

    assert_eq!(
        requests(&server).await.len(),
        2,
        "a 429 must cost exactly one extra attempt, not the full retry budget"
    );
    assert_eq!(summary.written, 1);
    assert_eq!(summary.failed, None);
    assert_eq!(
        world.entries()["item.testmod.copper"].value,
        translated("Copper Ingot")
    );
}

// ---------------------------------------------------------------------------
// 3. an auth failure writes nothing and is not retried
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_401_stops_the_run_without_writing_or_retrying() {
    let _g = test_lock();
    let world = world(&[("item.testmod.copper", "Copper Ingot")]);
    let server = MockServer::start().await;
    mount(&server, |_: &Request| {
        ResponseTemplate::new(401).set_body_string("invalid_api_key")
    })
    .await;

    let summary = prefill_uncancelled(&world, &server).await;

    assert_eq!(
        requests(&server).await.len(),
        1,
        "an auth failure cannot differ next time; retrying only delays the message"
    );
    assert_eq!(summary.written, 0);
    assert!(world.entries().is_empty(), "nothing may be written");
    let failed = summary
        .failed
        .expect("the failure is reported ON the summary");
    assert!(
        failed.contains("401") && failed.contains("local"),
        "the failure must say which provider answered how: {failed}"
    );
}

// ---------------------------------------------------------------------------
// 4. an unusable response body writes nothing at all
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_malformed_response_writes_nothing_not_even_partially() {
    let _g = test_lock();
    // Three strings in ONE batch (one role), so "partially" is a thing that
    // could happen if the parse failure were handled per string.
    let world = world(&[
        ("item.testmod.a", "Copper Ingot"),
        ("item.testmod.b", "Andesite Alloy"),
        ("item.testmod.c", "Cogwheel"),
    ]);
    let server = MockServer::start().await;
    // A misconfigured local server answering 200 with an HTML error page —
    // the realistic shape of "the transport worked, the body did not".
    mount(&server, |_: &Request| {
        ResponseTemplate::new(200).set_body_string("<html>502 Bad Gateway</html>")
    })
    .await;

    let summary = prefill_uncancelled(&world, &server).await;

    assert_eq!(summary.written, 0);
    assert!(
        world.entries().is_empty(),
        "an unparseable answer must leave the store untouched"
    );
    let failed = summary.failed.expect("the run failed");
    assert!(
        failed.contains("(0)"),
        "status 0 means the transport succeeded and the body did not: {failed}"
    );
    assert_eq!(
        requests(&server).await.len(),
        1,
        "an unparseable body is deterministic, so it must not be retried"
    );
}

// ---------------------------------------------------------------------------
// 5. ids nobody asked about buy nothing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_response_of_only_hallucinated_ids_writes_nothing_and_costs_no_retry() {
    let _g = test_lock();
    let world = world(&[
        ("item.testmod.a", "Copper Ingot"),
        ("item.testmod.b", "Andesite Alloy"),
        ("item.testmod.c", "Cogwheel"),
    ]);
    let server = MockServer::start().await;
    mount(&server, |_: &Request| {
        let out = BTreeMap::from([
            ("s99".to_string(), "Что-то".to_string()),
            ("item.other.mod".to_string(), "Ещё что-то".to_string()),
        ]);
        answer(&out, true)
    })
    .await;

    let summary = prefill_uncancelled(&world, &server).await;

    assert_eq!(
        summary.written, 0,
        "an id that was never requested must not become an override for a key \
         nobody asked about"
    );
    assert!(world.entries().is_empty());
    assert_eq!(
        summary.rejected, 0,
        "every requested id was MISSING, not refused — a distinction the retry \
         budget depends on"
    );
    assert_eq!(summary.failed, None, "omission is invited by the prompt");
    assert_eq!(
        requests(&server).await.len(),
        1,
        "a missing id gets no single-string retry; only a rejected one does"
    );
}

// ---------------------------------------------------------------------------
// 6. a string that cannot be verified is left English
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_mangled_placeholder_costs_one_retry_and_then_leaves_the_key_absent() {
    let _g = test_lock();
    let world = world(&[("item.testmod.uses", "Uses %s of %s")]);
    let server = MockServer::start().await;
    // Both placeholders dropped, twice — `prefill::verify` refuses it both
    // times, which is the point: a resource pack merges per key, so an absent
    // key falls back to the mod's own English at zero cost.
    mount(&server, |req: &Request| {
        let out = asked(req)
            .into_keys()
            .map(|id| (id, "Тратит ресурсы".to_string()))
            .collect();
        answer(&out, true)
    })
    .await;

    let summary = prefill_uncancelled(&world, &server).await;

    let seen = requests(&server).await;
    assert_eq!(
        seen.len(),
        2,
        "exactly one single-string retry: the batch, then one more chance"
    );
    let retry = asked(&seen[1]);
    assert_eq!(retry.len(), 1, "the retry asks about that one string only");

    let body: serde_json::Value = serde_json::from_slice(&seen[1].body).expect("JSON body");
    let user = body["messages"][1]["content"]
        .as_str()
        .expect("user message");
    assert!(
        user.contains("was rejected because") && user.contains("%s"),
        "a retry that does not say what was wrong is just 'try again': {user}"
    );

    assert_eq!(summary.rejected, 1);
    assert_eq!(summary.written, 0);
    assert_eq!(summary.failed, None, "a refusal is not a run failure");
    assert!(
        world.entries().is_empty(),
        "a string that cannot be verified is never written, so the mod's own \
         English still shows"
    );
}

// ---------------------------------------------------------------------------
// 7. cancel stops the spending
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_cancel_mid_run_keeps_what_was_bought_and_dispatches_nothing_further() {
    let _g = test_lock();
    // One key per role: three passes of one batch each. Tripping the flag while
    // the first batch is being answered means passes two and three must never
    // dispatch — an exact count, not a race, because a pass of one batch has
    // nothing else in flight.
    let world = world(&[
        ("item.testmod.copper", "Copper Ingot"),
        ("testmod.tooltip.hold", "Hold on"),
        ("gui.testmod.confirm", "Confirm"),
    ]);
    let server = MockServer::start().await;

    let cancel = Arc::new(AtomicBool::new(false));
    let trip = Arc::clone(&cancel);
    mount(&server, move |req: &Request| {
        trip.store(true, Ordering::SeqCst);
        translate_all(req, true)
    })
    .await;

    let summary = prefill(&world, &server, &cancel).await;

    assert_eq!(
        requests(&server).await.len(),
        1,
        "a cancelled run must not dispatch the batches it had not started"
    );
    assert!(summary.cancelled);
    assert_eq!(summary.failed, None, "a cancel is not a failure");
    assert_eq!(
        summary.written, 1,
        "what was already paid for and verified is still written"
    );
    let entries = world.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries["item.testmod.copper"].value,
        translated("Copper Ingot")
    );
}

// ---------------------------------------------------------------------------
// 8. a local model reports no usage, and that is not "free"
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_local_server_that_reports_no_usage_leaves_the_cost_unknown() {
    let _g = test_lock();
    let world = world(&[("item.testmod.copper", "Copper Ingot")]);
    let server = MockServer::start().await;
    // The same happy path, minus the `usage` object — which is what a local
    // OpenAI-compatible server typically omits.
    mount(&server, |req: &Request| translate_all(req, false)).await;

    let summary = prefill_uncancelled(&world, &server).await;

    assert_eq!(summary.written, 1);
    assert!(
        !summary.usage_known,
        "'this run was free' and 'we do not know what this run cost' are \
         different claims, and only one of them is true here"
    );
    assert_eq!(summary.prompt_tokens, 0);
    assert_eq!(summary.completion_tokens, 0);
    assert_eq!(
        world.entries()["item.testmod.copper"].value,
        translated("Copper Ingot")
    );
}

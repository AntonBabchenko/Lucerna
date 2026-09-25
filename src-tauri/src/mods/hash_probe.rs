//! Batch existence questions to Modrinth, keyed by file hash (2026-09-21 spec).
//!
//! Two questions answer what one `versions()` request per mod used to:
//! `version_files` — which version owns these bytes — and
//! `version_files/update_many` — the newest build of that project tagged for
//! (mc, loader). This module owns the TTL cache in front of them and the fetch
//! gate that makes concurrent passes join instead of sending the same requests
//! twice. It decides nothing about compatibility: `compat::batch_answer` does.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tokio::time::{Duration, Instant};

use crate::mods::modrinth::ModrinthClient;
use crate::mods::platform::{LoaderKind, ModVersion};

/// The `version_cache` figure: a platform's listings do not change because
/// something was installed locally.
const TTL: Duration = Duration::from_secs(5 * 60);

/// Why a batch request produced no answer. Neither is ever «no build».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchFailure {
    /// No usable answer: transport error, 5xx, 401/403, a 429 that outlived
    /// the chokepoint's retries. Every ask of the group is `Unreachable` —
    /// asking per project would fail the same way, once per mod.
    Unavailable,
    /// The platform answered and refused THIS REQUEST'S SHAPE (400, 404, 405,
    /// 410, 422) or sent a body that does not decode: the endpoint changed.
    /// The group falls back to the per-project listing.
    Unusable,
}

/// Status → failure class; `None` for 2xx. «Nothing known» is `200 {}` on both
/// endpoints (measured 2026-09-21), so a 404 is never a legitimate empty answer.
pub fn classify_status(status: u16) -> Option<BatchFailure> {
    match status {
        200..=299 => None,
        400 | 404 | 405 | 410 | 422 => Some(BatchFailure::Unusable),
        _ => Some(BatchFailure::Unavailable),
    }
}

/// The batch answers for one pass at one (mc, loader). Keys are lowercase sha1.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// `version_files`: the version owning these bytes; `None` = the platform
    /// does not know them.
    pub own: HashMap<String, Option<ModVersion>>,
    /// `update_many`: for each project owning these bytes, its newest version
    /// tagged for (mc, loader) — tags only, BEFORE our filename rule. Empty =
    /// none.
    pub latest: HashMap<String, Vec<ModVersion>>,
}

type LatestKey = (String, String, LoaderKind);

/// TTL cache + fetch gate. Production shares [`HashProbeCache::global`]; tests
/// build their own, so parallel tests never share state (the #428 lesson).
pub struct HashProbeCache {
    own: Mutex<HashMap<String, (Option<ModVersion>, Instant)>>,
    latest: Mutex<HashMap<LatestKey, (Vec<ModVersion>, Instant)>>,
    /// Held across a fetch: a concurrent pass waits, then reads the cache.
    gate: tokio::sync::Mutex<()>,
}

impl Default for HashProbeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl HashProbeCache {
    pub fn new() -> Self {
        Self {
            own: Mutex::new(HashMap::new()),
            latest: Mutex::new(HashMap::new()),
            gate: tokio::sync::Mutex::new(()),
        }
    }

    /// The process-wide cache the commands share.
    pub fn global() -> &'static HashProbeCache {
        static CACHE: OnceLock<HashProbeCache> = OnceLock::new();
        CACHE.get_or_init(HashProbeCache::new)
    }

    /// Both answers for `shas` at (`mc`, `loader`), from the cache where fresh,
    /// from the platform otherwise. Any failed request fails the call; what
    /// succeeded before it stays cached.
    pub async fn snapshot(
        &self,
        client: &ModrinthClient,
        shas: &[String],
        mc: &str,
        loader: LoaderKind,
    ) -> Result<Snapshot, BatchFailure> {
        // Lower-case once, at the boundary: the server's lookup is
        // case-sensitive (S9) and every key below uses this form.
        let mut wanted: Vec<String> = shas.iter().map(|s| s.to_ascii_lowercase()).collect();
        wanted.sort();
        wanted.dedup();
        if wanted.is_empty() {
            return Ok(Snapshot::default());
        }
        let _gate = self.gate.lock().await;
        // Expired entries go FIRST, under the gate: nothing present now can
        // disappear before this call assembles its answer, because only the
        // next call's `retain` removes entries and it waits on the same gate.
        let now = Instant::now();
        let missing_own: Vec<String> = {
            let mut own = self.own.lock().expect("hash probe cache mutex poisoned");
            own.retain(|_, (_, at)| now.saturating_duration_since(*at) < TTL);
            wanted
                .iter()
                .filter(|s| !own.contains_key(*s))
                .cloned()
                .collect()
        };
        if !missing_own.is_empty() {
            let got = client.owners_by_hashes(&missing_own).await?;
            let fetched_at = Instant::now();
            let mut own = self.own.lock().expect("hash probe cache mutex poisoned");
            for sha in missing_own {
                let v = got.get(&sha).cloned();
                own.insert(sha, (v, fetched_at));
            }
        }
        let latest_key = |sha: &str| (sha.to_string(), mc.to_string(), loader);
        let missing_latest: Vec<String> = {
            let mut latest = self.latest.lock().expect("hash probe cache mutex poisoned");
            latest.retain(|_, (_, at)| now.saturating_duration_since(*at) < TTL);
            wanted
                .iter()
                .filter(|s| !latest.contains_key(&latest_key(s)))
                .cloned()
                .collect()
        };
        if !missing_latest.is_empty() {
            let got = client.latest_by_hashes(&missing_latest, mc, loader).await?;
            let fetched_at = Instant::now();
            let mut latest = self.latest.lock().expect("hash probe cache mutex poisoned");
            for sha in missing_latest {
                let vs = got.get(&sha).cloned().unwrap_or_default();
                latest.insert(latest_key(&sha), (vs, fetched_at));
            }
        }
        let own = self.own.lock().expect("hash probe cache mutex poisoned");
        let latest = self.latest.lock().expect("hash probe cache mutex poisoned");
        let mut snap = Snapshot::default();
        for sha in &wanted {
            if let Some((v, _)) = own.get(sha) {
                snap.own.insert(sha.clone(), v.clone());
            }
            if let Some((vs, _)) = latest.get(&latest_key(sha)) {
                snap.latest.insert(sha.clone(), vs.clone());
            }
        }
        Ok(snap)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    /// A Modrinth v2 version object as the batch endpoints return it — only the
    /// fields `modrinth::types::Version` reads.
    pub(crate) fn version_json(
        id: &str,
        project: &str,
        mc: &str,
        loader: &str,
        filename: &str,
        sha1: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "project_id": project,
            "name": id,
            "version_number": id,
            "game_versions": [mc],
            "loaders": [loader],
            "date_published": "2026-09-01T00:00:00Z",
            "files": [{
                "url": "https://cdn.modrinth.com/data/x.jar",
                "filename": filename,
                "hashes": { "sha1": sha1 },
                "size": 1,
                "primary": true
            }],
            "dependencies": []
        })
    }

    /// A JSON object from `(key, value)` pairs — keys that are not literals.
    pub(crate) fn obj(entries: Vec<(&str, serde_json::Value)>) -> serde_json::Value {
        serde_json::Value::Object(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    /// The seam that lets a request reach a wiremock server on loopback.
    pub(crate) fn loopback_allowed() -> crate::test_seam::SeamScope {
        crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")])
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{loopback_allowed, obj, version_json};
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const H1: &str = "459b5f4c7297b2f7649d43137f3e5a069b69b707";
    const NF: LoaderKind = LoaderKind::NeoForge;

    async fn empty_answers(s: &MockServer, expect: u64) {
        for p in ["/v2/version_files", "/v2/version_files/update_many"] {
            Mock::given(method("POST"))
                .and(path(p))
                .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
                .expect(expect)
                .mount(s)
                .await;
        }
    }

    #[test]
    fn statuses_split_into_unavailable_and_unusable() {
        for ok in [200u16, 204] {
            assert_eq!(classify_status(ok), None, "{ok}");
        }
        for shape in [400u16, 404, 405, 410, 422] {
            assert_eq!(
                classify_status(shape),
                Some(BatchFailure::Unusable),
                "{shape}"
            );
        }
        for down in [401u16, 403, 429, 500, 502, 503] {
            assert_eq!(
                classify_status(down),
                Some(BatchFailure::Unavailable),
                "{down}"
            );
        }
    }

    #[tokio::test]
    async fn a_snapshot_asks_each_endpoint_once_and_then_answers_from_the_cache() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(obj(vec![(
                H1,
                version_json("v1", "p1", "1.21.1", "neoforge", "p1-1.0.jar", H1),
            )])))
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files/update_many"))
            .respond_with(ResponseTemplate::new(200).set_body_json(obj(vec![(
                H1,
                serde_json::json!([version_json(
                    "v2",
                    "p1",
                    "1.21.1",
                    "neoforge",
                    "p1-1.1.jar",
                    "ffff"
                )]),
            )])))
            .expect(1)
            .mount(&s)
            .await;
        let _seam = loopback_allowed();
        let cache = HashProbeCache::new();
        let client = ModrinthClient::with_base(s.uri());

        // Upper-case in, lower-case keys out: the server's lookup is case-sensitive.
        let snap = cache
            .snapshot(&client, &[H1.to_ascii_uppercase()], "1.21.1", NF)
            .await
            .unwrap();
        assert_eq!(
            snap.own[H1].as_ref().map(|v| v.version_id.as_str()),
            Some("v1")
        );
        assert_eq!(snap.latest[H1][0].version_id, "v2");

        // Inside the TTL nothing is sent again (`expect(1)` is verified on drop).
        let again = cache
            .snapshot(&client, &[H1.to_string()], "1.21.1", NF)
            .await
            .unwrap();
        assert_eq!(again.latest[H1][0].version_id, "v2");
    }

    #[tokio::test]
    async fn more_than_a_chunk_of_hashes_takes_two_requests_per_endpoint() {
        let s = MockServer::start().await;
        empty_answers(&s, 2).await;
        let _seam = loopback_allowed();
        let shas: Vec<String> = (0..101).map(|i| format!("{i:040x}")).collect();
        let snap = HashProbeCache::new()
            .snapshot(&ModrinthClient::with_base(s.uri()), &shas, "1.21.1", NF)
            .await
            .unwrap();
        // Unknown bytes are remembered AS unknown, not dropped.
        assert_eq!(snap.own.len(), 101);
        assert!(snap.own.values().all(Option::is_none));
        assert_eq!(snap.latest.len(), 101);
    }

    #[tokio::test]
    async fn two_concurrent_snapshots_send_one_set_of_requests() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("{}")
                    .set_delay(std::time::Duration::from_millis(200)),
            )
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files/update_many"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&s)
            .await;
        let _seam = loopback_allowed();
        let cache = HashProbeCache::new();
        let client = ModrinthClient::with_base(s.uri());
        let shas = vec![H1.to_string()];
        let (a, b) = tokio::join!(
            cache.snapshot(&client, &shas, "1.21.1", NF),
            cache.snapshot(&client, &shas, "1.21.1", NF),
        );
        assert!(a.is_ok() && b.is_ok());
    }

    #[tokio::test]
    async fn a_failed_batch_is_not_cached() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&s)
            .await;
        empty_answers(&s, 1).await;
        let _seam = loopback_allowed();
        let cache = HashProbeCache::new();
        let client = ModrinthClient::with_base(s.uri());
        let shas = vec![H1.to_string()];
        assert_eq!(
            cache
                .snapshot(&client, &shas, "1.21.1", NF)
                .await
                .unwrap_err(),
            BatchFailure::Unavailable
        );
        assert!(
            cache.snapshot(&client, &shas, "1.21.1", NF).await.is_ok(),
            "a failure must not be remembered"
        );
    }

    #[tokio::test]
    async fn a_body_that_does_not_decode_is_unusable_not_unavailable() {
        // (pin under the stub)
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>maintenance</html>"))
            .mount(&s)
            .await;
        let _seam = loopback_allowed();
        let got = HashProbeCache::new()
            .snapshot(
                &ModrinthClient::with_base(s.uri()),
                &[H1.to_string()],
                "1.21.1",
                NF,
            )
            .await;
        assert_eq!(got.unwrap_err(), BatchFailure::Unusable);
    }

    #[tokio::test]
    async fn no_hashes_no_requests() {
        let s = MockServer::start().await;
        empty_answers(&s, 0).await;
        let _seam = loopback_allowed();
        let snap = HashProbeCache::new()
            .snapshot(&ModrinthClient::with_base(s.uri()), &[], "1.21.1", NF)
            .await
            .unwrap();
        assert!(snap.own.is_empty() && snap.latest.is_empty());
    }
}

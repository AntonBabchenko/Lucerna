//! The network half of the compatibility pass (2026-09-21 spec, §5.3/§6): the
//! hash batch where its answer is provably the listing's, today's per-project
//! listing for everything else. Extracted from
//! `commands::mods::gather_compat_facts` so it can be tested without an
//! AppHandle — the client, the cache and the listing are passed in.

use std::future::Future;

use futures_util::stream::{self, StreamExt};

use crate::mods::compat::{
    batch_answer, live_availability, settle_without_listing, InstalledFile, LiveAvailability,
    ProbeAnswer,
};
use crate::mods::hash_probe::{BatchFailure, HashProbeCache};
use crate::mods::modrinth::ModrinthClient;
use crate::mods::platform::{LoaderKind, ModSource, ModVersion};

/// Residue listings in flight at once — the bound the per-mod probe has always
/// used: dozens of simultaneous requests intermittently trip per-IP limits.
const LISTING_CONCURRENCY: usize = 6;

fn file_of(a: &Ask) -> InstalledFile<'_> {
    InstalledFile {
        on_disk_sha1: a.on_disk_sha1.as_deref(),
        registry_sha1: &a.registry_sha1,
        registry_version_id: a.registry_version_id.as_deref(),
    }
}

/// One existence question: is this installed file listed for (mc, `loader`)?
#[derive(Debug, Clone)]
pub struct Ask {
    pub source: ModSource,
    pub project_id: String,
    /// The loader that actually opens the jar (`local::probe_loader`).
    pub loader: LoaderKind,
    /// `installed::on_disk_sha1` — lowercase hex; `None` = could not tell.
    pub on_disk_sha1: Option<String>,
    pub registry_sha1: String,
    pub registry_version_id: Option<String>,
    /// The jar's own verdict is `Fits` — then «builds exist» settles it (§5.3).
    pub verdict_fits: bool,
}

/// One answered question, index-aligned with the asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub availability: LiveAvailability,
    /// The newest listed version number, when builds are listed.
    pub newest: Option<String>,
}

/// Answer every ask. `listing` is the per-project fallback (in production:
/// `versions()` through the coalescing version cache); `on_tick(done, total)`
/// opens the phase with `(0, total)` and then reports every settled ask.
pub async fn resolve_asks<L, Fut, T>(
    asks: &[Ask],
    mc: &str,
    client: &ModrinthClient,
    cache: &HashProbeCache,
    listing: L,
    mut on_tick: T,
) -> Vec<Resolved>
where
    L: Fn(ModSource, String, LoaderKind) -> Fut + Sync,
    Fut: Future<Output = crate::error::Result<Vec<ModVersion>>> + Send,
    T: FnMut(u32, u32) + Send,
{
    // safe: an instance's mod list is nowhere near 2^32 entries.
    let total = asks.len() as u32;
    if total == 0 {
        return Vec::new();
    }
    on_tick(0, total);
    let mut out: Vec<Option<Resolved>> = vec![None; asks.len()];
    let mut residue: Vec<usize> = Vec::new();

    // 1. Modrinth asks with a known on-disk digest: one batch group per probe
    //    loader. Everything else goes straight to the listing.
    let mut groups: Vec<(LoaderKind, Vec<usize>)> = Vec::new();
    for (i, a) in asks.iter().enumerate() {
        if a.source != ModSource::Modrinth || a.on_disk_sha1.is_none() {
            residue.push(i);
            continue;
        }
        match groups.iter_mut().find(|(l, _)| *l == a.loader) {
            Some((_, g)) => g.push(i),
            None => groups.push((a.loader, vec![i])),
        }
    }
    let mut done: u32 = 0;
    for (loader, group) in groups {
        let shas: Vec<String> = group
            .iter()
            .filter_map(|&i| asks[i].on_disk_sha1.clone())
            .collect();
        match cache.snapshot(client, &shas, mc, loader).await {
            Ok(snap) => {
                for &i in &group {
                    let a = &asks[i];
                    let key = a
                        .on_disk_sha1
                        .as_deref()
                        .map(str::to_ascii_lowercase)
                        .unwrap_or_default();
                    // An asked hash missing from the snapshot declines — a
                    // missing `latest` must never read as «nothing listed».
                    let (Some(own), Some(latest)) = (snap.own.get(&key), snap.latest.get(&key))
                    else {
                        residue.push(i);
                        continue;
                    };
                    let answer =
                        batch_answer(&file_of(a), &a.project_id, mc, loader, own.as_ref(), latest);
                    match settle_without_listing(&answer, a.verdict_fits) {
                        Some((availability, newest)) => {
                            out[i] = Some(Resolved {
                                availability,
                                newest,
                            });
                            done += 1;
                        }
                        None => residue.push(i),
                    }
                }
            }
            // Asked and failed: the whole group is `Unreachable` — never
            // «no build», and asking per project would fail the same way.
            Err(BatchFailure::Unavailable) => {
                for &i in &group {
                    out[i] = Some(Resolved {
                        availability: LiveAvailability::Unreachable,
                        newest: None,
                    });
                    done += 1;
                }
            }
            // The platform refused the request's shape: today's path.
            Err(BatchFailure::Unusable) => residue.extend(group.iter().copied()),
        }
    }
    if done > 0 {
        on_tick(done, total);
    }

    // 2. The residue: the per-project listing, as before this change.
    let mut pending = std::pin::pin!(stream::iter(residue)
        .map(|i| {
            let a = &asks[i];
            let fut = listing(a.source, a.project_id.clone(), a.loader);
            async move {
                let file = file_of(a);
                let r = match fut.await {
                    Ok(versions) => Resolved {
                        availability: live_availability(&file, ProbeAnswer::Found(&versions)),
                        newest: versions.first().map(|v| v.version_number.clone()),
                    },
                    // A failed query must never read as «no build».
                    Err(_) => Resolved {
                        availability: live_availability(&file, ProbeAnswer::Failed),
                        newest: None,
                    },
                };
                (i, r)
            }
        })
        .buffer_unordered(LISTING_CONCURRENCY));
    while let Some((i, r)) = pending.next().await {
        out[i] = Some(r);
        done += 1;
        on_tick(done, total);
    }
    out.into_iter()
        // Every index was filled above; «could not tell» is the safe reading
        // if that ever stops being true.
        .map(|r| {
            r.unwrap_or(Resolved {
                availability: LiveAvailability::Unreachable,
                newest: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::compat::batch_model::build;
    use crate::mods::hash_probe::test_support::{loopback_allowed, obj, version_json};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const NF: LoaderKind = LoaderKind::NeoForge;
    const MR: ModSource = ModSource::Modrinth;

    fn ask(source: ModSource, project: &str, h: Option<&str>, fits: bool) -> Ask {
        Ask {
            source,
            project_id: project.into(),
            loader: NF,
            on_disk_sha1: h.map(str::to_string),
            registry_sha1: h.unwrap_or("00").into(),
            registry_version_id: None,
            verdict_fits: fits,
        }
    }

    fn resolved(availability: LiveAvailability, newest: Option<&str>) -> Resolved {
        Resolved {
            availability,
            newest: newest.map(str::to_string),
        }
    }

    async fn answers(s: &MockServer, owners: serde_json::Value, latest: serde_json::Value) {
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(owners))
            .mount(s)
            .await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files/update_many"))
            .respond_with(ResponseTemplate::new(200).set_body_json(latest))
            .mount(s)
            .await;
    }

    async fn status(s: &MockServer, endpoint: &str, code: u16) {
        Mock::given(method("POST"))
            .and(path(endpoint))
            .respond_with(ResponseTemplate::new(code))
            .mount(s)
            .await;
    }

    /// A per-project listing that counts its calls and answers from `table`
    /// (an absent project lists nothing).
    fn listing<'a>(
        calls: &'a AtomicUsize,
        table: &'a [(&'a str, Vec<ModVersion>)],
    ) -> impl Fn(
        ModSource,
        String,
        LoaderKind,
    ) -> std::future::Ready<crate::error::Result<Vec<ModVersion>>>
           + Sync
           + 'a {
        move |_, project, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            let found = table
                .iter()
                .find(|(p, _)| *p == project)
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            std::future::ready(Ok(found))
        }
    }

    fn v1_listed() -> serde_json::Value {
        version_json("v1", "p1", "1.21.1", "neoforge", "p1-1.jar", "aa")
    }

    #[tokio::test]
    async fn the_batch_settles_what_it_can_prove_without_a_single_listing() {
        let s = MockServer::start().await;
        answers(
            &s,
            obj(vec![
                ("aa", v1_listed()),
                (
                    "bb",
                    version_json("w1", "p2", "1.21", "neoforge", "p2-1.jar", "bb"),
                ),
            ]),
            obj(vec![("aa", serde_json::json!([v1_listed()]))]),
        )
        .await;
        let _seam = loopback_allowed();
        let calls = AtomicUsize::new(0);
        let asks = vec![
            ask(MR, "p1", Some("aa"), false),
            ask(MR, "p2", Some("bb"), false),
        ];
        let got = resolve_asks(
            &asks,
            "1.21.1",
            &ModrinthClient::with_base(s.uri()),
            &HashProbeCache::new(),
            listing(&calls, &[]),
            |_, _| {},
        )
        .await;
        assert_eq!(
            got,
            vec![
                resolved(LiveAvailability::FileListed, Some("v1")),
                resolved(LiveAvailability::NoBuilds, None),
            ]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn an_undecided_ask_costs_exactly_one_listing_and_the_ticks_say_so() {
        let s = MockServer::start().await;
        answers(
            &s,
            obj(vec![("aa", v1_listed())]),
            obj(vec![("aa", serde_json::json!([v1_listed()]))]),
        )
        .await;
        let _seam = loopback_allowed();
        let calls = AtomicUsize::new(0);
        let mut ticks = Vec::new();
        // "cc" is unknown to the platform: B1 — only the listing can answer.
        let asks = vec![
            ask(MR, "p1", Some("aa"), false),
            ask(MR, "p3", Some("cc"), false),
        ];
        let got = resolve_asks(
            &asks,
            "1.21.1",
            &ModrinthClient::with_base(s.uri()),
            &HashProbeCache::new(),
            listing(&calls, &[]),
            |d, t| ticks.push((d, t)),
        )
        .await;
        assert_eq!(got[1], resolved(LiveAvailability::NoBuilds, None));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(ticks, vec![(0, 2), (1, 2), (2, 2)]);
    }

    #[tokio::test]
    async fn builds_listed_settles_a_fits_jar_and_lists_an_undeclared_one() {
        let s = MockServer::start().await;
        answers(
            &s,
            obj(vec![
                (
                    "aa",
                    version_json("v1", "p1", "1.21", "neoforge", "p1-1.jar", "aa"),
                ),
                (
                    "ee",
                    version_json("x1", "p4", "1.21", "neoforge", "p4-1.jar", "ee"),
                ),
            ]),
            obj(vec![
                (
                    "aa",
                    serde_json::json!([version_json(
                        "v2", "p1", "1.21.1", "neoforge", "p1-2.jar", "dd"
                    )]),
                ),
                (
                    "ee",
                    serde_json::json!([version_json(
                        "x2", "p4", "1.21.1", "neoforge", "p4-2.jar", "ff"
                    )]),
                ),
            ]),
        )
        .await;
        let _seam = loopback_allowed();
        let calls = AtomicUsize::new(0);
        let table = [(
            "p4",
            vec![build("p4", "x2", &["1.21.1"], &[NF], "p4-2.jar", &["ff"]).version],
        )];
        let asks = vec![
            ask(MR, "p1", Some("aa"), true),
            ask(MR, "p4", Some("ee"), false),
        ];
        let got = resolve_asks(
            &asks,
            "1.21.1",
            &ModrinthClient::with_base(s.uri()),
            &HashProbeCache::new(),
            listing(&calls, &table),
            |_, _| {},
        )
        .await;
        assert_eq!(
            got,
            vec![
                resolved(LiveAvailability::OtherBuildsOnly, Some("v2")),
                resolved(LiveAvailability::OtherBuildsOnly, Some("x2")),
            ]
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "only the undeclared jar is listed"
        );
    }

    #[tokio::test]
    async fn an_unavailable_batch_makes_the_group_unreachable_and_never_lists() {
        // (pin under the stub)
        let s = MockServer::start().await;
        status(&s, "/v2/version_files", 503).await;
        let _seam = loopback_allowed();
        let calls = AtomicUsize::new(0);
        let asks = vec![
            ask(MR, "p1", Some("aa"), false),
            ask(MR, "p2", Some("bb"), true),
        ];
        let got = resolve_asks(
            &asks,
            "1.21.1",
            &ModrinthClient::with_base(s.uri()),
            &HashProbeCache::new(),
            listing(&calls, &[]),
            |_, _| {},
        )
        .await;
        assert!(got
            .iter()
            .all(|r| r.availability == LiveAvailability::Unreachable));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_failed_update_many_never_reads_as_no_build() {
        // (pin under the stub)
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(obj(vec![(
                "aa",
                version_json("v1", "p1", "1.21", "neoforge", "p1-1.jar", "aa"),
            )])))
            .mount(&s)
            .await;
        status(&s, "/v2/version_files/update_many", 503).await;
        let _seam = loopback_allowed();
        let calls = AtomicUsize::new(0);
        let got = resolve_asks(
            &[ask(MR, "p1", Some("aa"), false)],
            "1.21.1",
            &ModrinthClient::with_base(s.uri()),
            &HashProbeCache::new(),
            listing(&calls, &[]),
            |_, _| {},
        )
        .await;
        assert_eq!(got, vec![resolved(LiveAvailability::Unreachable, None)]);
    }

    #[tokio::test]
    async fn a_refused_request_shape_falls_back_to_the_listing() {
        let s = MockServer::start().await;
        status(&s, "/v2/version_files", 400).await;
        let _seam = loopback_allowed();
        let calls = AtomicUsize::new(0);
        let asks = vec![
            ask(MR, "p1", Some("aa"), false),
            ask(MR, "p2", Some("bb"), true),
        ];
        let got = resolve_asks(
            &asks,
            "1.21.1",
            &ModrinthClient::with_base(s.uri()),
            &HashProbeCache::new(),
            listing(&calls, &[]),
            |_, _| {},
        )
        .await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "every ask of the group is listed"
        );
        assert!(got
            .iter()
            .all(|r| r.availability == LiveAvailability::NoBuilds));
    }

    #[tokio::test]
    async fn a_curseforge_ask_never_enters_the_batch() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(0)
            .mount(&s)
            .await;
        let _seam = loopback_allowed();
        let calls = AtomicUsize::new(0);
        let got = resolve_asks(
            &[ask(ModSource::Curseforge, "123", Some("aa"), false)],
            "1.21.1",
            &ModrinthClient::with_base(s.uri()),
            &HashProbeCache::new(),
            listing(&calls, &[]),
            |_, _| {},
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(got, vec![resolved(LiveAvailability::NoBuilds, None)]);
    }

    #[tokio::test]
    async fn duplicate_files_get_the_same_answer_by_index() {
        let s = MockServer::start().await;
        for (p, body) in [
            ("/v2/version_files", obj(vec![("aa", v1_listed())])),
            (
                "/v2/version_files/update_many",
                obj(vec![("aa", serde_json::json!([v1_listed()]))]),
            ),
        ] {
            Mock::given(method("POST"))
                .and(path(p))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .expect(1)
                .mount(&s)
                .await;
        }
        let _seam = loopback_allowed();
        let calls = AtomicUsize::new(0);
        // `a.jar` and `a (1).jar` — the same bytes twice.
        let asks = vec![
            ask(MR, "p1", Some("aa"), false),
            ask(MR, "p1", Some("aa"), false),
        ];
        let got = resolve_asks(
            &asks,
            "1.21.1",
            &ModrinthClient::with_base(s.uri()),
            &HashProbeCache::new(),
            listing(&calls, &[]),
            |_, _| {},
        )
        .await;
        assert_eq!(
            got,
            vec![resolved(LiveAvailability::FileListed, Some("v1")); 2]
        );
    }
}

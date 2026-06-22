//! Regression tests for the two live bugs the resolver-kernel work fixed,
//! exercised through the real public API against a mock Modrinth server.
//!
//! R1 — modId ≠ slug: a dependency's loader id (`spore`) differs from its
//! published Modrinth slug (`fungal-infection-spore`). The old code looked up
//! the loader id as a slug, missed, and dead-ended. `modrinth_lookup` must now
//! recover it: the exact-slug `versions("spore")` miss falls through to a
//! relevance `search("spore")`, whose hit is accepted by `name_matches`, and a
//! follow-up `versions(project_id)` returns the build.
//!
//! R2 — author pin honored over newer: a required dependency that pins an older
//! `version_id` must install that exact build (and stamp `PinHonored`), not the
//! newest compatible build. (The pure decision is unit-tested in
//! `mods::dep_select`; the modrinth client glue has its own in-crate anchor
//! `resolve_deps_honors_author_pinned_version_id`. This is the durable
//! end-to-end regression at the integration layer.)
//!
//! These run single-threaded by convention with the other env-seam wiremock
//! integration tests; each scopes the host allowlist to the mock server only.

use lucerna_lib::mods::dep_resolve::modrinth_lookup;
use lucerna_lib::mods::modrinth::ModrinthClient;
use lucerna_lib::mods::platform::{
    DepKind, DepProjectRef, LoaderKind, ModDepLink, ModFile, ModPlatform, ModSource, ModVersion,
    SelectionReason,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// R1: the loader id `spore` ≠ the published slug `fungal-infection-spore`.
/// The exact-slug lookup misses; the relevance search recovers it by name.
#[tokio::test]
async fn modid_not_equal_slug_resolves_via_name_search() {
    let server = MockServer::start().await;

    // 1. Exact-slug lookup for the bare loader id misses (slug ≠ id).
    Mock::given(method("GET"))
        .and(path("/v2/project/spore/version"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;

    // 2. Relevance search returns the real project under its published slug.
    Mock::given(method("GET"))
        .and(path("/v2/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{
              "hits": [{
                "project_id":"FIS","slug":"fungal-infection-spore",
                "title":"Fungal Infection: Spore","description":"Zombies",
                "icon_url":null,"downloads":4200,"author":"giselbaer",
                "date_modified":"2026-05-01T00:00:00Z"
              }],
              "total_hits":1,"offset":0,"limit":20
            }"#,
        ))
        .mount(&server)
        .await;

    // 3. Versions for the recovered project id return a compatible build.
    Mock::given(method("GET"))
        .and(path("/v2/project/FIS/version"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"[{
              "id":"fis-1","project_id":"FIS","name":"Spore 1.20.1","version_number":"2.4.0",
              "game_versions":["1.20.1"],"loaders":["forge"],"date_published":"2026-05-01T00:00:00Z",
              "files":[{"url":"https://cdn/spore.jar","filename":"spore-forge-1.20.1-2.4.0.jar",
                        "hashes":{"sha1":"abc"},"size":100,"primary":true}],
              "dependencies":[]
            }]"#,
        ))
        .mount(&server)
        .await;

    let _seam =
        lucerna_lib::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
    let client = ModrinthClient::with_base(server.uri());

    let versions = modrinth_lookup(&client, "spore", "1.20.1", LoaderKind::Forge).await;

    assert_eq!(
        versions.len(),
        1,
        "the slug-miss must be recovered by the name search, got: {versions:?}"
    );
    assert_eq!(versions[0].project_id, "FIS");
    assert_eq!(versions[0].version_id, "fis-1");
    assert_eq!(
        versions[0].primary_file.filename,
        "spore-forge-1.20.1-2.4.0.jar"
    );
}

/// Sanity guard for R1: when the search hit's name/slug does NOT match the dep
/// id, `name_matches` rejects it and the lookup stays empty — proving the
/// recovery is gated by a real match, not by "any first hit".
#[tokio::test]
async fn modid_not_equal_slug_unrelated_search_hit_is_rejected() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/project/spore/version"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    // Search returns a hit whose slug/name share nothing with "spore".
    Mock::given(method("GET"))
        .and(path("/v2/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{
              "hits": [{
                "project_id":"JEI","slug":"jei","title":"Just Enough Items",
                "description":"Items","icon_url":null,"downloads":9,"author":"mezz",
                "date_modified":"2026-05-01T00:00:00Z"
              }],
              "total_hits":1,"offset":0,"limit":20
            }"#,
        ))
        .mount(&server)
        .await;

    let _seam =
        lucerna_lib::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
    let client = ModrinthClient::with_base(server.uri());

    let versions = modrinth_lookup(&client, "spore", "1.20.1", LoaderKind::Forge).await;
    assert!(
        versions.is_empty(),
        "an unrelated search hit must not be accepted, got: {versions:?}"
    );
}

/// R2: a required dep pinning an older `version_id` installs that exact build,
/// not the newest compatible one, and stamps `PinHonored`.
#[tokio::test]
async fn author_pin_is_installed_over_newer_build() {
    let server = MockServer::start().await;
    // The dep project "lib" has two compatible builds, newest-first. The author
    // pinned the OLDER build ("lib-old").
    Mock::given(method("GET"))
        .and(path("/v2/project/lib/version"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"[
              {"id":"lib-new","project_id":"lib","name":"Lib 2","version_number":"2.0.0",
               "game_versions":["1.20.1"],"loaders":["forge"],"date_published":"2026-05-01T00:00:00Z",
               "files":[{"url":"https://cdn/new.jar","filename":"lib-2.jar","hashes":{"sha1":"bb"},"size":1,"primary":true}],
               "dependencies":[]},
              {"id":"lib-old","project_id":"lib","name":"Lib 1","version_number":"1.0.0",
               "game_versions":["1.20.1"],"loaders":["forge"],"date_published":"2026-01-01T00:00:00Z",
               "files":[{"url":"https://cdn/old.jar","filename":"lib-1.jar","hashes":{"sha1":"cc"},"size":1,"primary":true}],
               "dependencies":[]}
            ]"#,
        ))
        .mount(&server)
        .await;

    let _seam =
        lucerna_lib::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
    let client = ModrinthClient::with_base(server.uri());

    let primary = ModVersion {
        source: ModSource::Modrinth,
        project_id: "primary".into(),
        version_id: "vp".into(),
        name: "Primary".into(),
        version_number: "1.0".into(),
        mc_versions: vec!["1.20.1".into()],
        loaders: vec![LoaderKind::Forge],
        primary_file: ModFile {
            filename: "p.jar".into(),
            url: "https://cdn/p.jar".into(),
            sha1: Some("aa".into()),
            size: 1.0,
            distribution_allowed: true,
        },
        deps: vec![ModDepLink {
            kind: DepKind::Required,
            project_ref: DepProjectRef::Modrinth {
                project_id: "lib".into(),
                version_id: Some("lib-old".into()),
            },
        }],
        published_at: None,
    };

    let resolved = client
        .resolve_deps(&primary, "1.20.1", LoaderKind::Forge)
        .await
        .expect("resolve_deps succeeds against the mock");

    assert_eq!(resolved.required.len(), 1, "{resolved:?}");
    assert_eq!(
        resolved.required[0].version.version_id, "lib-old",
        "the author-pinned older build must win over the newer one"
    );
    assert_eq!(
        resolved.required[0].selection_reason,
        SelectionReason::PinHonored
    );
}

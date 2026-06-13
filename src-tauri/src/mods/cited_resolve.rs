//! Best-effort resolution of a `CitedMod` (a bare mod-id from a server
//! reject log) to a downloadable Modrinth/CurseForge version. mod-id is
//! NOT a platform slug/id, so resolution is fuzzy and tiered:
//!   Exact      — an unambiguous slug match with a compatible version.
//!   Fuzzy      — plausible hits, user picks.
//!   Unresolved — nothing usable; user searches manually.
//! Pure tier-selection lives here; the async orchestration (search + versions)
//! is `resolve()` in this same file.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::logs::diagnose::server_mods::{CitedKind, CitedMod};
use crate::mods::platform::{
    drop_filename_loader_mismatches, ContentKind, InstalledMod, LoaderKind, ModPlatform,
    ModSearchQuery, ModSort, ModSource, ModSummary, ModVersion, VersionRef,
};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResolvedCandidate {
    pub target: VersionRef,
    pub display: ModSummary,
    /// Human label, e.g. the version_number "0.5.3".
    pub version_label: String,
    /// Display names of the candidate's REQUIRED dependencies (transitive
    /// closure), so the card can show what else will be installed. Empty when
    /// the mod is standalone or deps couldn't be resolved.
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "tier", rename_all = "snake_case")]
pub enum ResolveTier {
    Exact { candidate: Box<ResolvedCandidate> },
    Fuzzy { candidates: Vec<ResolvedCandidate> },
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResolvedMod {
    pub cited: CitedMod,
    pub tier: ResolveTier,
}

/// True when a search hit is an unambiguous match for the cited id: its
/// slug equals the id case-insensitively. (`project_id` is never compared —
/// mod-ids are not platform ids.)
pub fn is_exact_slug_match(cited_id: &str, hit: &ModSummary) -> bool {
    hit.slug
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case(cited_id))
        .unwrap_or(false)
}

/// Pick the version to install from a project's compatible versions. When
/// `want_version` is set (mismatch case), prefer the exact `version_number`;
/// otherwise (and as fallback) take the first (newest) compatible version.
/// `versions` MUST already be filtered to the instance's mc+loader by the
/// caller. Returns `None` only when `versions` is empty.
pub fn pick_version<'a>(
    versions: &'a [ModVersion],
    want_version: Option<&str>,
) -> Option<&'a ModVersion> {
    if let Some(w) = want_version {
        if let Some(v) = versions.iter().find(|v| v.version_number == w) {
            return Some(v);
        }
    }
    versions.first()
}

/// Build a candidate from a hit + chosen version + resolved dependency names.
pub fn make_candidate(
    hit: &ModSummary,
    version: &ModVersion,
    dependencies: Vec<String>,
) -> ResolvedCandidate {
    ResolvedCandidate {
        target: VersionRef {
            source: version.source,
            project_id: version.project_id.clone(),
            version_id: version.version_id.clone(),
        },
        display: hit.clone(),
        version_label: version.version_number.clone(),
        dependencies,
    }
}

/// Resolve the REQUIRED-dependency display names for a picked version, so the
/// UI can show what else an install will pull in. Best-effort: returns empty
/// on any resolve failure (the install itself still pulls deps regardless).
/// Names come from each dep's project (clean name), falling back to the dep
/// version's own name. Deduped, order-preserving.
async fn required_dep_names(
    plat: &dyn ModPlatform,
    version: &ModVersion,
    mc_version: &str,
    loader: LoaderKind,
) -> Vec<String> {
    let Ok(deps) = plat.resolve_deps(version, mc_version, loader).await else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for d in &deps.required {
        let name = match plat.project(&d.version.project_id).await {
            Ok(p) => p.summary.name,
            Err(_) => d.version.name.clone(),
        };
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

const MAX_FUZZY: usize = 5;

/// Normalize a mod identifier for loose matching: ascii-alphanumerics only,
/// lowercased. "Farmer's Delight" → "farmersdelight".
fn normalize_id(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Lowercased alphanumeric tokens of a filename, split on any separator.
/// "FarmersDelight-1.20.1-1.3.2.jar" → ["farmersdelight","1","20","1","1","3","2","jar"].
fn filename_tokens(filename: &str) -> impl Iterator<Item = String> + '_ {
    filename
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
}

/// Heuristic, OFFLINE check (no network): is a cited mod-id already satisfied
/// by an installed mod? `InstalledMod` does not store the jar's internal
/// mod-id, so we match the normalized cited id against the installed mod's
/// name, its `project_id`, and its filename tokens. Powers "don't re-warn /
/// re-offer a mod the user already installed" — the reject log is historical
/// and never changes. Token-matching the filename (not prefix) avoids a
/// false positive like cited "create" matching "createdeco-1.0.jar".
pub fn cited_satisfied_by_installed(cited_id: &str, installed: &[InstalledMod]) -> bool {
    let target = normalize_id(cited_id);
    if target.is_empty() {
        return false;
    }
    installed.iter().any(|m| {
        normalize_id(&m.name) == target
            || m.project_id.as_deref().map(normalize_id).as_deref() == Some(target.as_str())
            || filename_tokens(&m.filename).any(|t| t == target)
    })
}

/// The subset of `cited` not yet satisfied by an installed mod.
pub fn unsatisfied_cited(cited: &[CitedMod], installed: &[InstalledMod]) -> Vec<CitedMod> {
    cited
        .iter()
        .filter(|c| !cited_satisfied_by_installed(&c.id, installed))
        .cloned()
        .collect()
}

/// Resolve every cited mod to a tier. Network: one search + (per candidate)
/// one versions() call, faceted by mc+loader. `installed` powers the
/// high-confidence mismatch path (skip search when the project_id is known).
/// Modrinth is consulted first; CurseForge is the fallback when Modrinth
/// yields nothing for a cited id.
///
/// Honest invariant: a candidate is only ever offered with a version that is
/// compatible with `mc_version` + `loader` (the platform facets it, and we
/// additionally drop filename-loader mis-tags). No usable version → Unresolved.
pub async fn resolve(
    cited: &[CitedMod],
    mc_version: &str,
    loader: LoaderKind,
    modrinth: &dyn ModPlatform,
    curseforge: &dyn ModPlatform,
    installed: &[InstalledMod],
) -> Vec<ResolvedMod> {
    let mut out = Vec::with_capacity(cited.len());
    for c in cited {
        let tier = resolve_one(c, mc_version, loader, modrinth, curseforge, installed).await;
        out.push(ResolvedMod {
            cited: c.clone(),
            tier,
        });
    }
    out
}

async fn resolve_one(
    c: &CitedMod,
    mc_version: &str,
    loader: LoaderKind,
    modrinth: &dyn ModPlatform,
    curseforge: &dyn ModPlatform,
    installed: &[InstalledMod],
) -> ResolveTier {
    // 1. High-confidence mismatch path: the mod is already installed with a
    //    known platform identity → resolve the wanted version directly.
    if c.kind == CitedKind::VersionMismatch {
        if let Some(m) = installed.iter().find(|m| {
            m.project_id.is_some()
                && (m.name.eq_ignore_ascii_case(&c.id)
                    || m.project_id
                        .as_deref()
                        .is_some_and(|p| p.eq_ignore_ascii_case(&c.id)))
        }) {
            if let (Some(source), Some(pid)) = (m.source, m.project_id.clone()) {
                let plat: &dyn ModPlatform = match source {
                    ModSource::Curseforge => curseforge,
                    _ => modrinth,
                };
                if let Ok(versions) = plat.versions(&pid, Some(mc_version), Some(loader)).await {
                    let versions = drop_filename_loader_mismatches(versions, Some(loader));
                    if let Some(v) = pick_version(&versions, c.version.as_deref()) {
                        let summary = ModSummary {
                            source,
                            project_id: pid.clone(),
                            slug: Some(c.id.clone()),
                            name: m.name.clone(),
                            summary: String::new(),
                            icon_url: None,
                            downloads: 0.0,
                            author: String::new(),
                            updated_at: None,
                        };
                        let deps = required_dep_names(plat, v, mc_version, loader).await;
                        return ResolveTier::Exact {
                            candidate: Box::new(make_candidate(&summary, v, deps)),
                        };
                    }
                }
            }
        }
    }

    // 2. Search path (Missing, or mismatch without identity). Modrinth first,
    //    CurseForge as fallback when Modrinth resolves nothing.
    for (plat, source) in [
        (modrinth, ModSource::Modrinth),
        (curseforge, ModSource::Curseforge),
    ] {
        let tier = resolve_via_search(c, mc_version, loader, plat, source).await;
        if !matches!(tier, ResolveTier::Unresolved) {
            return tier;
        }
    }
    ResolveTier::Unresolved
}

async fn resolve_via_search(
    c: &CitedMod,
    mc_version: &str,
    loader: LoaderKind,
    plat: &dyn ModPlatform,
    source: ModSource,
) -> ResolveTier {
    let q = ModSearchQuery {
        source,
        kind: ContentKind::Mod,
        query: c.id.clone(),
        mc_version: Some(mc_version.to_string()),
        loader: Some(loader),
        sort: ModSort::Relevance,
        page_size: MAX_FUZZY as u32,
        offset: 0,
    };
    let Ok(page) = plat.search(&q).await else {
        return ResolveTier::Unresolved;
    };
    if page.hits.is_empty() {
        return ResolveTier::Unresolved;
    }

    // Exact: a slug-equal hit that has a compatible version.
    if let Some(hit) = page.hits.iter().find(|h| is_exact_slug_match(&c.id, h)) {
        if let Ok(versions) = plat
            .versions(&hit.project_id, Some(mc_version), Some(loader))
            .await
        {
            let versions = drop_filename_loader_mismatches(versions, Some(loader));
            if let Some(v) = pick_version(&versions, c.version.as_deref()) {
                let deps = required_dep_names(plat, v, mc_version, loader).await;
                return ResolveTier::Exact {
                    candidate: Box::new(make_candidate(hit, v, deps)),
                };
            }
        }
    }

    // Fuzzy: up to MAX_FUZZY hits that each have a compatible version.
    let mut candidates = Vec::new();
    for hit in page.hits.iter().take(MAX_FUZZY) {
        if let Ok(versions) = plat
            .versions(&hit.project_id, Some(mc_version), Some(loader))
            .await
        {
            let versions = drop_filename_loader_mismatches(versions, Some(loader));
            if let Some(v) = pick_version(&versions, c.version.as_deref()) {
                let deps = required_dep_names(plat, v, mc_version, loader).await;
                candidates.push(make_candidate(hit, v, deps));
            }
        }
    }
    if candidates.is_empty() {
        ResolveTier::Unresolved
    } else {
        ResolveTier::Fuzzy { candidates }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::diagnose::server_mods::CitedKind;
    use crate::mods::platform::{LoaderKind, ModFile, ModSource};

    fn hit(slug: Option<&str>, pid: &str) -> ModSummary {
        ModSummary {
            source: ModSource::Modrinth,
            project_id: pid.into(),
            slug: slug.map(Into::into),
            name: "X".into(),
            summary: "".into(),
            icon_url: None,
            downloads: 1.0,
            author: "a".into(),
            updated_at: None,
        }
    }

    fn ver(num: &str) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: "pid".into(),
            version_id: format!("v-{num}"),
            name: num.into(),
            version_number: num.into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![LoaderKind::Forge],
            primary_file: ModFile {
                filename: format!("x-{num}.jar"),
                url: "https://example/x.jar".into(),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
            },
            deps: vec![],
            published_at: None,
        }
    }

    fn fake_installed(name: &str, filename: &str, project: Option<&str>) -> InstalledMod {
        InstalledMod {
            filename: filename.into(),
            sha1: "sha".into(),
            source: project.map(|_| ModSource::Modrinth),
            project_id: project.map(Into::into),
            version_id: project.map(|_| "v".into()),
            name: name.into(),
            version_number: Some("1.0".into()),
            installed_at: "2026-01-01T00:00:00Z".into(),
            enabled: true,
            enrich_attempted: false,
            requires: vec![],
        }
    }

    #[test]
    fn cited_satisfied_matches_by_name_or_filename_token() {
        let installed = vec![
            fake_installed("Farmer's Delight", "FarmersDelight-1.20.1-1.3.2.jar", None),
            fake_installed(
                "Just Enough Items",
                "jei-1.20.1-forge-15.20.0.130.jar",
                Some("u6dRKJwZ"),
            ),
        ];
        assert!(cited_satisfied_by_installed("farmersdelight", &installed)); // normalized name
        assert!(cited_satisfied_by_installed("jei", &installed)); // filename token
        assert!(!cited_satisfied_by_installed("create", &installed));
    }

    #[test]
    fn cited_satisfied_no_false_positive_on_substring() {
        // cited "create" must NOT be satisfied by "Create Deco" / createdeco jar.
        let installed = vec![fake_installed("Create Deco", "createdeco-1.0.jar", None)];
        assert!(!cited_satisfied_by_installed("create", &installed));
    }

    #[test]
    fn unsatisfied_cited_drops_installed_keeps_missing() {
        let installed = vec![fake_installed(
            "Farmer's Delight",
            "FarmersDelight-1.3.2.jar",
            None,
        )];
        let cited = vec![
            CitedMod {
                id: "farmersdelight".into(),
                version: None,
                kind: CitedKind::Missing,
            },
            CitedMod {
                id: "create".into(),
                version: None,
                kind: CitedKind::Missing,
            },
        ];
        let out = unsatisfied_cited(&cited, &installed);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "create");
    }

    #[test]
    fn exact_slug_match_is_case_insensitive() {
        assert!(is_exact_slug_match("JEI", &hit(Some("jei"), "p")));
        assert!(!is_exact_slug_match(
            "jei",
            &hit(Some("just-enough-items"), "p")
        ));
        assert!(!is_exact_slug_match("jei", &hit(None, "p")));
    }

    #[test]
    fn pick_version_prefers_exact_then_newest() {
        let vs = vec![ver("0.5.3"), ver("0.5.2"), ver("0.4.0")];
        assert_eq!(
            pick_version(&vs, Some("0.4.0")).unwrap().version_number,
            "0.4.0"
        );
        assert_eq!(
            pick_version(&vs, Some("9.9.9")).unwrap().version_number,
            "0.5.3"
        );
        assert_eq!(pick_version(&vs, None).unwrap().version_number, "0.5.3");
        assert!(pick_version(&[], None).is_none());
    }

    // ── async wiremock tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn missing_mod_with_exact_slug_resolves_to_exact_tier() {
        let _env = crate::test_env_lock();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");

        use crate::logs::diagnose::server_mods::{CitedKind, CitedMod};
        use crate::mods::modrinth::ModrinthClient;
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let s = MockServer::start().await;

        // Mock search: returns one hit with slug "jei"
        Mock::given(method("GET"))
            .and(path("/v2/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                    "hits": [{
                        "project_id": "u6dRKJwZ",
                        "slug": "jei",
                        "title": "JEI",
                        "description": "Just Enough Items",
                        "icon_url": null,
                        "downloads": 5000000,
                        "author": "mezz",
                        "date_modified": "2026-05-01T00:00:00Z"
                    }],
                    "total_hits": 1,
                    "offset": 0,
                    "limit": 5
                }"#,
            ))
            .mount(&s)
            .await;

        // Mock versions: returns one compatible 1.20.1/forge build
        Mock::given(method("GET"))
            .and(path("/v2/project/u6dRKJwZ/version"))
            .and(query_param("loaders", r#"["forge"]"#))
            .and(query_param("game_versions", r#"["1.20.1"]"#))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{
                    "id": "jei-forge-v1",
                    "project_id": "u6dRKJwZ",
                    "name": "JEI 15.3.0",
                    "version_number": "15.3.0",
                    "game_versions": ["1.20.1"],
                    "loaders": ["forge"],
                    "date_published": "2026-05-01T00:00:00Z",
                    "files": [{
                        "url": "https://cdn.modrinth.com/jei-forge-15.3.0.jar",
                        "filename": "jei-forge-1.20.1-15.3.0.jar",
                        "hashes": {"sha1": "deadbeef"},
                        "size": 2048,
                        "primary": true
                    }],
                    "dependencies": []
                }]"#,
            ))
            .mount(&s)
            .await;

        let mr = ModrinthClient::with_base(s.uri());
        let cited = vec![CitedMod {
            id: "jei".into(),
            version: None,
            kind: CitedKind::Missing,
        }];

        let out = resolve(&cited, "1.20.1", LoaderKind::Forge, &mr, &mr, &[]).await;
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        assert_eq!(out.len(), 1);
        match &out[0].tier {
            ResolveTier::Exact { candidate } => {
                assert_eq!(candidate.target.project_id, "u6dRKJwZ");
                assert_eq!(candidate.target.version_id, "jei-forge-v1");
                assert_eq!(candidate.version_label, "15.3.0");
            }
            other => panic!("expected Exact, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_mod_resolves_to_unresolved() {
        let _env = crate::test_env_lock();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");

        use crate::logs::diagnose::server_mods::{CitedKind, CitedMod};
        use crate::mods::modrinth::ModrinthClient;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let s = MockServer::start().await;

        // Both modrinth and curseforge handles point to the same mock server
        // which returns empty hits for any search.
        Mock::given(method("GET"))
            .and(path("/v2/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                    "hits": [],
                    "total_hits": 0,
                    "offset": 0,
                    "limit": 5
                }"#,
            ))
            .mount(&s)
            .await;

        let mr = ModrinthClient::with_base(s.uri());
        let cited = vec![CitedMod {
            id: "ghostmod".into(),
            version: None,
            kind: CitedKind::Missing,
        }];

        let out = resolve(&cited, "1.20.1", LoaderKind::Forge, &mr, &mr, &[]).await;
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        assert_eq!(out.len(), 1);
        assert!(
            matches!(out[0].tier, ResolveTier::Unresolved),
            "expected Unresolved, got {:?}",
            out[0].tier
        );
    }

    #[tokio::test]
    async fn candidate_dependencies_surfaced_from_required_dep() {
        let _env = crate::test_env_lock();
        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");

        use crate::logs::diagnose::server_mods::{CitedKind, CitedMod};
        use crate::mods::modrinth::ModrinthClient;
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let s = MockServer::start().await;

        // Search: returns a hit for the compat-patch mod.
        Mock::given(method("GET"))
            .and(path("/v2/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                    "hits": [{
                        "project_id": "patch123",
                        "slug": "fd-ntp-compat",
                        "title": "FD x NTP Cooking Pot",
                        "description": "Compat patch",
                        "icon_url": null,
                        "downloads": 1000,
                        "author": "patchauthor",
                        "date_modified": "2026-05-01T00:00:00Z"
                    }],
                    "total_hits": 1,
                    "offset": 0,
                    "limit": 5
                }"#,
            ))
            .mount(&s)
            .await;

        // Versions for the compat-patch: one build with a required dep on "ntpmod".
        Mock::given(method("GET"))
            .and(path("/v2/project/patch123/version"))
            .and(query_param("loaders", r#"["forge"]"#))
            .and(query_param("game_versions", r#"["1.20.1"]"#))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{
                    "id": "patch-v1",
                    "project_id": "patch123",
                    "name": "FD-NTP 1.0.0",
                    "version_number": "1.0.0",
                    "game_versions": ["1.20.1"],
                    "loaders": ["forge"],
                    "date_published": "2026-05-01T00:00:00Z",
                    "files": [{
                        "url": "https://cdn.modrinth.com/patch.jar",
                        "filename": "fd-ntp-compat-1.0.0.jar",
                        "hashes": {"sha1": "aabbcc"},
                        "size": 512,
                        "primary": true
                    }],
                    "dependencies": [
                        {"project_id": "ntpmod", "dependency_type": "required"}
                    ]
                }]"#,
            ))
            .mount(&s)
            .await;

        // resolve_deps calls versions for each required dep.
        Mock::given(method("GET"))
            .and(path("/v2/project/ntpmod/version"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{
                    "id": "ntp-v1",
                    "project_id": "ntpmod",
                    "name": "No Tree Punching",
                    "version_number": "7.0.0",
                    "game_versions": ["1.20.1"],
                    "loaders": ["forge"],
                    "date_published": "2026-05-01T00:00:00Z",
                    "files": [{
                        "url": "https://cdn.modrinth.com/ntp.jar",
                        "filename": "notreepunching-forge-7.0.0.jar",
                        "hashes": {"sha1": "ccddee"},
                        "size": 256,
                        "primary": true
                    }],
                    "dependencies": []
                }]"#,
            ))
            .mount(&s)
            .await;

        // project() call for the dep to get its display name.
        Mock::given(method("GET"))
            .and(path("/v2/project/ntpmod"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                    "id": "ntpmod",
                    "slug": "no-tree-punching",
                    "title": "No Tree Punching",
                    "description": "NTP mod",
                    "icon_url": null,
                    "downloads": 50000,
                    "team": "teamA",
                    "published": "2026-01-01T00:00:00Z",
                    "updated": "2026-05-01T00:00:00Z",
                    "body": ""
                }"#,
            ))
            .mount(&s)
            .await;

        let mr = ModrinthClient::with_base(s.uri());
        let cited = vec![CitedMod {
            id: "fd-ntp-compat".into(),
            version: None,
            kind: CitedKind::Missing,
        }];

        let out = resolve(&cited, "1.20.1", LoaderKind::Forge, &mr, &mr, &[]).await;
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        assert_eq!(out.len(), 1);
        match &out[0].tier {
            ResolveTier::Exact { candidate } => {
                assert_eq!(candidate.target.project_id, "patch123");
                assert_eq!(
                    candidate.dependencies,
                    vec!["No Tree Punching"],
                    "required dep name should be surfaced from project title"
                );
            }
            other => panic!("expected Exact, got {other:?}"),
        }
    }
}

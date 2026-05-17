//! Quilt loader meta client.
//!
//! Endpoints:
//! - `https://meta.quiltmc.org/v3/versions/loader/<mc>` →
//!   `[{ loader: { version, build, maven, separator } }, …]`
//! - `https://meta.quiltmc.org/v3/versions/loader/<mc>/<loader>/profile/json` →
//!   Mojang-format VersionDetails with `inheritsFrom`.
//!
//! Quilt meta does NOT expose a `stable` boolean on each loader entry,
//! and its `build` field is just the LAST numeric component of the
//! version string (`0.20.0-beta.9` → `build=9`, `0.17.11` → `build=11`,
//! `0.29.0` → `build=0`) — not a globally increasing counter as in
//! Fabric meta. Sorting by `build` puts `0.17.11` ahead of `0.29.0`,
//! which is wrong.
//!
//! Conventions used here:
//!   - Stable detection: version string with no `-` qualifier
//!     (`0.23.1` → stable, `0.24.0-beta.1` → not).
//!   - Sort order: parse each version into a SemVer-shaped tuple
//!     `(MAJOR, MINOR, PATCH, is_release, beta_or_release_num)` and
//!     sort descending. Newest goes to the top.
//!   - "Recommended" tag: only the SINGLE topmost stable entry gets
//!     `stable=true`; the rest are downgraded to `stable=false`. Quilt
//!     has no promotions feed, so we treat "newest stable" as the one
//!     recommended version, matching Forge's promotions semantic.

use crate::error::{Error, Result};
use crate::network::get_json;
use crate::versions::loaders::LoaderVersion;
use crate::versions::version_json::{parse, Library, VersionDetails};
use serde::Deserialize;

const META_DEFAULT: &str = "https://meta.quiltmc.org";

fn meta_base() -> String {
    std::env::var("FTLAUNCHER_QUILT_META_OVERRIDE")
        .unwrap_or_else(|_| META_DEFAULT.to_string())
}

#[derive(Debug, Deserialize)]
struct RawEntry {
    loader: RawLoader,
    /// Quilt's own SRG-like mappings (`org.quiltmc:hashed:<mc>`).
    /// Newer Quilt loaders (0.20+) require this AND `intermediary` at
    /// runtime but no longer carry them in `profile/json`'s libraries —
    /// the launcher must inject them. Null when Quilt hasn't yet
    /// published mappings for the given MC version (e.g. brand-new
    /// snapshots) — such combinations CRASH at startup with "Requested
    /// target namespace intermediary not loaded", so we filter them out
    /// of the dropdown.
    #[serde(default)]
    hashed: Option<RawMapping>,
    /// Fabric intermediary mappings (`net.fabricmc:intermediary:<mc>`).
    /// Same nullability semantics as `hashed`.
    #[serde(default)]
    intermediary: Option<RawMapping>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawMapping {
    maven: String,
}

#[derive(Debug, Deserialize)]
struct RawLoader {
    version: String,
    build: u32,
}

pub(super) async fn list(mc: &str) -> Result<Vec<LoaderVersion>> {
    let url = format!("{}/v3/versions/loader/{mc}", meta_base());
    let raw: Vec<RawEntry> = get_json(&url, "loaders/quilt").await?;
    // Filter out entries Quilt can't actually launch — when their meta has
    // no `hashed`/`intermediary` mappings published yet (typical for the
    // first weeks after a new MC release), the loader crashes immediately
    // with "Requested target namespace intermediary not loaded". Better to
    // hide such combinations than to let the user pick a guaranteed-broken
    // version.
    let mut out: Vec<LoaderVersion> = raw
        .into_iter()
        .filter(|e| e.hashed.is_some() && e.intermediary.is_some())
        .map(|e| {
            let stable = !e.loader.version.contains('-');
            LoaderVersion {
                version: e.loader.version,
                stable,
                build: e.loader.build,
            }
        })
        .collect();
    // Sort by semver-shaped tuple descending (newest first).
    out.sort_by(|a, b| sort_key(&b.version).cmp(&sort_key(&a.version)));
    // Mark only the newest stable as "recommended" — the rest are still
    // installable but the UI drops their tag, matching Forge's
    // promotions semantic (one anointed version per MC).
    let mut first_stable_seen = false;
    for v in out.iter_mut() {
        if v.stable {
            if first_stable_seen {
                v.stable = false;
            } else {
                first_stable_seen = true;
            }
        }
    }
    Ok(out)
}

/// Parse a Quilt loader version string into a sort key. Higher tuple
/// → newer version. Tuple layout: `(major, minor, patch, is_release,
/// trailing_number)`. `is_release=true` ranks ABOVE the corresponding
/// beta at the same `major.minor.patch` (so `0.20.0` > `0.20.0-beta.9`).
fn sort_key(version: &str) -> (u32, u32, u32, bool, u32) {
    // Split off optional `-<qualifier>` suffix.
    let (base, qualifier) = match version.split_once('-') {
        Some((b, q)) => (b, Some(q)),
        None => (version, None),
    };
    let mut parts = base.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);

    // Tie-breaker among betas of the same patch: the trailing number
    // (e.g. `beta.9` → 9). Released (`is_release=true`) versions ignore
    // it; we set trailing to 0.
    let (is_release, trailing) = match qualifier {
        None => (true, 0u32),
        Some(q) => {
            let n = q
                .rsplit('.')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0u32);
            (false, n)
        }
    };
    (major, minor, patch, is_release, trailing)
}

pub(super) async fn profile(mc: &str, ver: &str) -> Result<VersionDetails> {
    let url = format!("{}/v3/versions/loader/{mc}/{ver}/profile/json", meta_base());
    let raw_json: serde_json::Value = get_json(&url, "loaders/quilt").await?;
    let text = serde_json::to_string(&raw_json)
        .map_err(|e| Error::io(url.clone(), format!("serialise: {e}")))?;
    let mut details = parse(&text).map_err(|e| Error::io(url, format!("parse: {e}")))?;

    // Newer Quilt loaders (0.20+) require `hashed` + `intermediary` mappings
    // at runtime but no longer ship them in `profile/json`'s libraries[].
    // The launcher must add them from the meta-list entry's sibling fields.
    // For older loaders these libs are also in profile/json, so we dedup by
    // maven coord prefix to avoid spurious classpath duplicates.
    let mappings = fetch_mapping_coords(mc, ver).await?;
    for maven in mappings {
        let already_present = details
            .libraries
            .iter()
            .any(|l| same_maven_artifact(&l.name, &maven));
        if !already_present {
            details.libraries.push(Library {
                name: maven.clone(),
                url: Some(maven_url_for_mapping(&maven).into()),
                downloads: None,
                rules: None,
                natives: None,
            });
        }
    }
    Ok(details)
}

/// Pick the maven host for a Quilt-side mapping coord. `org.quiltmc:hashed:*`
/// lives on Quilt's maven; `net.fabricmc:intermediary:*` lives on Fabric's.
/// Both are already in the allowlist (Quilt for the loader itself, Fabric
/// for sponge-mixin/asm libraries already listed in profile.json).
fn maven_url_for_mapping(coord: &str) -> &'static str {
    if coord.starts_with("net.fabricmc:") {
        "https://maven.fabricmc.net/"
    } else {
        "https://maven.quiltmc.org/repository/release/"
    }
}

/// Fetch the per-MC loader list and pull the `hashed.maven` +
/// `intermediary.maven` coords for the requested loader version.
/// Returns 0-2 coords (both may be present, both may be absent).
async fn fetch_mapping_coords(mc: &str, ver: &str) -> Result<Vec<String>> {
    let url = format!("{}/v3/versions/loader/{mc}", meta_base());
    let raw: Vec<RawEntry> = get_json(&url, "loaders/quilt/mappings").await?;
    let entry = raw
        .into_iter()
        .find(|e| e.loader.version == ver)
        .ok_or_else(|| Error::LoaderUnavailable {
            loader: "quilt".to_string(),
            mc_version: mc.to_string(),
        })?;
    let mut out = Vec::new();
    if let Some(h) = entry.hashed {
        out.push(h.maven);
    }
    if let Some(i) = entry.intermediary {
        out.push(i.maven);
    }
    Ok(out)
}

/// Compare two maven coordinates ignoring trailing classifier/version
/// nuances when deciding "is this artifact already present". Two coords
/// match if they share `group:artifact` (ignoring version + classifier).
fn same_maven_artifact(a: &str, b: &str) -> bool {
    let key = |c: &str| {
        let mut it = c.split(':');
        let g = it.next().unwrap_or("");
        let a = it.next().unwrap_or("");
        format!("{g}:{a}")
    };
    key(a) == key(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Serializes env-var tests: they mutate FTLAUNCHER_QUILT_META_OVERRIDE
    // and FTLAUNCHER_EXTRA_ALLOWED_HOSTS, which are process-global and
    // shared across cargo's parallel test threads. Without this, two tests
    // running in parallel both flip the override → false negatives.
    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    const FIXTURE_LIST: &str = r#"[
      {
        "loader":{"separator":".","build":1,"maven":"org.quiltmc:quilt-loader:0.23.1","version":"0.23.1"},
        "hashed":{"maven":"org.quiltmc:hashed:1.20.4"},
        "intermediary":{"maven":"net.fabricmc:intermediary:1.20.4"}
      },
      {
        "loader":{"separator":".","build":2,"maven":"org.quiltmc:quilt-loader:0.24.0-beta.1","version":"0.24.0-beta.1"},
        "hashed":{"maven":"org.quiltmc:hashed:1.20.4"},
        "intermediary":{"maven":"net.fabricmc:intermediary:1.20.4"}
      }
    ]"#;

    /// Matches real Quilt meta API shape: loader profiles do NOT include
    /// `assetIndex`, `assets`, or `downloads`.
    const FIXTURE_PROFILE: &str = r#"{
      "id": "quilt-loader-0.23.1-1.20.4",
      "inheritsFrom": "1.20.4",
      "type": "release",
      "mainClass": "org.quiltmc.loader.impl.launch.knot.KnotClient",
      "libraries": [
        {"name":"org.quiltmc:quilt-loader:0.23.1","url":"https://maven.quiltmc.org/repository/release/"},
        {"name":"org.quiltmc:hashed:1.20.4+build.1","url":"https://maven.quiltmc.org/repository/release/"}
      ],
      "arguments": {"jvm": [], "game": []}
    }"#;

    #[tokio::test]
    async fn list_against_wiremock_marks_dashed_versions_unstable() {
        let _guard = env_lock();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v3/versions/loader/1.20.4"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_LIST))
            .mount(&server)
            .await;

        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var("FTLAUNCHER_QUILT_META_OVERRIDE", server.uri());

        let out = list("1.20.4").await.expect("list");
        assert_eq!(out.len(), 2);
        // Sorted by semver tuple desc — beta 0.24.0-beta.1 ranks AFTER
        // stable 0.23.1 because 0.24.0-beta is pre-release of 0.24.0
        // which beats 0.23.1 on patch ordering.
        // (0, 24, 0, false, 1) > (0, 23, 1, true, 0): patch differs → 24>23
        assert_eq!(out[0].version, "0.24.0-beta.1");
        assert!(!out[0].stable);
        assert_eq!(out[1].version, "0.23.1");
        // Topmost stable → keeps stable=true (only one stable here).
        assert!(out[1].stable);

        std::env::remove_var("FTLAUNCHER_QUILT_META_OVERRIDE");
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
    }

    #[test]
    fn sort_key_orders_minor_versions_correctly() {
        // The original `build`-based sort put 0.17.11 (build=11) ahead of
        // 0.29.0 (build=0). Verify the semver-tuple sort fixes this.
        assert!(sort_key("0.29.0") > sort_key("0.17.11"));
        assert!(sort_key("0.20.1") > sort_key("0.20.0"));
        assert!(sort_key("0.17.11") > sort_key("0.17.1"));
    }

    #[test]
    fn sort_key_stable_beats_beta_at_same_patch() {
        // (0, 20, 0, true, 0) > (0, 20, 0, false, 9): is_release wins.
        assert!(sort_key("0.20.0") > sort_key("0.20.0-beta.9"));
    }

    #[test]
    fn sort_key_higher_patch_beats_stable_at_lower_patch() {
        // 0.29.2-beta.1 has patch 2 → newer than 0.29.0 even though it's beta.
        assert!(sort_key("0.29.2-beta.1") > sort_key("0.29.0"));
    }

    #[test]
    fn sort_key_beta_tie_breaker_uses_trailing_number() {
        assert!(sort_key("0.20.0-beta.9") > sort_key("0.20.0-beta.1"));
    }

    #[tokio::test]
    async fn list_marks_only_newest_stable_as_recommended() {
        let _guard = env_lock();
        // 4 versions: 2 stables + 2 betas. Only the newest stable
        // (0.29.0 — beats 0.23.1) should retain stable=true after the
        // "recommended downgrade" pass; the older 0.23.1 becomes stable=false.
        // All four have hashed+intermediary (supported MC).
        let fixture = r#"[
          {"loader":{"separator":".","build":0,"maven":"x:y:0.29.0","version":"0.29.0"},
            "hashed":{"maven":"org.quiltmc:hashed:1.21.11"},
            "intermediary":{"maven":"net.fabricmc:intermediary:1.21.11"}},
          {"loader":{"separator":".","build":1,"maven":"x:y:0.23.0-beta.1","version":"0.23.0-beta.1"},
            "hashed":{"maven":"org.quiltmc:hashed:1.21.11"},
            "intermediary":{"maven":"net.fabricmc:intermediary:1.21.11"}},
          {"loader":{"separator":".","build":1,"maven":"x:y:0.23.1","version":"0.23.1"},
            "hashed":{"maven":"org.quiltmc:hashed:1.21.11"},
            "intermediary":{"maven":"net.fabricmc:intermediary:1.21.11"}},
          {"loader":{"separator":".","build":9,"maven":"x:y:0.20.0-beta.9","version":"0.20.0-beta.9"},
            "hashed":{"maven":"org.quiltmc:hashed:1.21.11"},
            "intermediary":{"maven":"net.fabricmc:intermediary:1.21.11"}}
        ]"#;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v3/versions/loader/1.21.11"))
            .respond_with(ResponseTemplate::new(200).set_body_string(fixture))
            .mount(&server)
            .await;
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var("FTLAUNCHER_QUILT_META_OVERRIDE", server.uri());

        let out = list("1.21.11").await.expect("list");
        assert_eq!(out.len(), 4);
        // Order: 0.29.0 > 0.23.1 > 0.23.0-beta.1 > 0.20.0-beta.9
        assert_eq!(out[0].version, "0.29.0");
        assert!(out[0].stable, "newest stable must keep stable=true");
        assert_eq!(out[1].version, "0.23.1");
        assert!(!out[1].stable, "older stable must be demoted to stable=false");
        assert_eq!(out[2].version, "0.23.0-beta.1");
        assert!(!out[2].stable);
        assert_eq!(out[3].version, "0.20.0-beta.9");
        assert!(!out[3].stable);

        std::env::remove_var("FTLAUNCHER_QUILT_META_OVERRIDE");
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
    }

    #[test]
    fn profile_fixture_parses() {
        let v = parse(FIXTURE_PROFILE).expect("parse quilt profile");
        assert_eq!(v.id, "quilt-loader-0.23.1-1.20.4");
        assert_eq!(v.inherits_from.as_deref(), Some("1.20.4"));
        assert!(v.main_class.contains("KnotClient"));
        assert_eq!(v.libraries.len(), 2);
        for lib in &v.libraries {
            assert!(lib.downloads.is_none());
            assert!(lib.url.is_some());
        }
        // Real Quilt API does not include these fields — vanilla parent provides them.
        assert!(v.asset_index.is_none(), "loader profile must not have assetIndex");
        assert!(v.assets.is_none(), "loader profile must not have assets");
        assert!(v.downloads.is_none(), "loader profile must not have downloads");
    }

    #[tokio::test]
    async fn list_filters_out_entries_without_mappings() {
        // Brand-new MC version that Quilt's meta lists a loader for but
        // hasn't published mappings for yet — `hashed`/`intermediary` are
        // null. Such combos crash on launch with "Requested target
        // namespace intermediary not loaded"; we hide them from the
        // dropdown.
        let _guard = env_lock();
        let fixture = r#"[
          {"loader":{"separator":".","build":2,"maven":"x:y:0.29.2","version":"0.29.2"}},
          {"loader":{"separator":".","build":0,"maven":"x:y:0.28.0","version":"0.28.0"},
            "hashed":null, "intermediary":null},
          {"loader":{"separator":".","build":1,"maven":"x:y:0.27.0","version":"0.27.0"},
            "hashed":{"maven":"org.quiltmc:hashed:1.21.11"},
            "intermediary":{"maven":"net.fabricmc:intermediary:1.21.11"}}
        ]"#;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v3/versions/loader/26.1.2"))
            .respond_with(ResponseTemplate::new(200).set_body_string(fixture))
            .mount(&server)
            .await;
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var("FTLAUNCHER_QUILT_META_OVERRIDE", server.uri());

        let out = list("26.1.2").await.expect("list");
        // Only 0.27.0 has both mappings populated; 0.29.2 (missing keys)
        // and 0.28.0 (explicit nulls) are filtered out.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].version, "0.27.0");

        std::env::remove_var("FTLAUNCHER_QUILT_META_OVERRIDE");
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
    }

    #[tokio::test]
    async fn profile_injects_meta_mappings_when_profile_omits_them() {
        // Newer Quilt loaders (0.20+) ship mappings via the meta-list
        // entry's `hashed`/`intermediary` siblings, NOT in profile.json's
        // libraries. The launcher must inject them so the loader finds
        // intermediary at runtime.
        let _guard = env_lock();
        const PROFILE_NO_MAPPINGS: &str = r#"{
          "id":"quilt-loader-0.29.2-1.21.11",
          "inheritsFrom":"1.21.11",
          "type":"release",
          "mainClass":"org.quiltmc.loader.impl.launch.knot.KnotClient",
          "libraries":[
            {"name":"org.quiltmc:quilt-loader:0.29.2","url":"https://maven.quiltmc.org/repository/release/"}
          ],
          "arguments":{"jvm":[],"game":[]}
        }"#;
        const LIST_ONE_ENTRY: &str = r#"[
          {"loader":{"separator":".","build":2,"maven":"x:y:0.29.2","version":"0.29.2"},
            "hashed":{"maven":"org.quiltmc:hashed:1.21.11"},
            "intermediary":{"maven":"net.fabricmc:intermediary:1.21.11"}}
        ]"#;

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v3/versions/loader/1.21.11/0.29.2/profile/json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(PROFILE_NO_MAPPINGS))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v3/versions/loader/1.21.11"))
            .respond_with(ResponseTemplate::new(200).set_body_string(LIST_ONE_ENTRY))
            .mount(&server)
            .await;
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var("FTLAUNCHER_QUILT_META_OVERRIDE", server.uri());

        let v = profile("1.21.11", "0.29.2").await.expect("profile");
        // quilt-loader + hashed + intermediary = 3.
        assert_eq!(v.libraries.len(), 3, "got {:?}", v.libraries.iter().map(|l| &l.name).collect::<Vec<_>>());
        let names: Vec<&str> = v.libraries.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"org.quiltmc:hashed:1.21.11"));
        assert!(names.contains(&"net.fabricmc:intermediary:1.21.11"));
        // intermediary should be hinted at fabricmc maven, hashed at quiltmc.
        for lib in &v.libraries {
            if lib.name.starts_with("net.fabricmc:intermediary") {
                assert_eq!(lib.url.as_deref(), Some("https://maven.fabricmc.net/"));
            }
            if lib.name.starts_with("org.quiltmc:hashed") {
                assert_eq!(lib.url.as_deref(), Some("https://maven.quiltmc.org/repository/release/"));
            }
        }

        std::env::remove_var("FTLAUNCHER_QUILT_META_OVERRIDE");
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
    }

    #[tokio::test]
    async fn profile_skips_mapping_inject_when_already_present() {
        // Older Quilt loaders (0.17.x) ship mappings IN profile.json's
        // libraries. We must NOT duplicate them when the meta-list entry
        // also exposes the same coords.
        let _guard = env_lock();
        const PROFILE_WITH_MAPPINGS: &str = r#"{
          "id":"quilt-loader-0.17.11-1.21.11",
          "inheritsFrom":"1.21.11",
          "type":"release",
          "mainClass":"org.quiltmc.loader.impl.launch.knot.KnotClient",
          "libraries":[
            {"name":"org.quiltmc:quilt-loader:0.17.11","url":"https://maven.quiltmc.org/repository/release/"},
            {"name":"org.quiltmc:hashed:1.21.11","url":"https://maven.quiltmc.org/repository/release/"},
            {"name":"net.fabricmc:intermediary:1.21.11","url":"https://maven.fabricmc.net/"}
          ],
          "arguments":{"jvm":[],"game":[]}
        }"#;
        const LIST_ONE_ENTRY: &str = r#"[
          {"loader":{"separator":".","build":11,"maven":"x:y:0.17.11","version":"0.17.11"},
            "hashed":{"maven":"org.quiltmc:hashed:1.21.11"},
            "intermediary":{"maven":"net.fabricmc:intermediary:1.21.11"}}
        ]"#;

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v3/versions/loader/1.21.11/0.17.11/profile/json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(PROFILE_WITH_MAPPINGS))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v3/versions/loader/1.21.11"))
            .respond_with(ResponseTemplate::new(200).set_body_string(LIST_ONE_ENTRY))
            .mount(&server)
            .await;
        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var("FTLAUNCHER_QUILT_META_OVERRIDE", server.uri());

        let v = profile("1.21.11", "0.17.11").await.expect("profile");
        // 3 libraries → no duplicates injected.
        assert_eq!(v.libraries.len(), 3);

        std::env::remove_var("FTLAUNCHER_QUILT_META_OVERRIDE");
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
    }
}

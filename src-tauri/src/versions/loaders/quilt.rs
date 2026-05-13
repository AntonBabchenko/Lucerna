//! Quilt loader meta client.
//!
//! Endpoints:
//! - `https://meta.quiltmc.org/v3/versions/loader/<mc>` →
//!   `[{ loader: { version, build, maven, separator } }, …]`
//! - `https://meta.quiltmc.org/v3/versions/loader/<mc>/<loader>/profile/json` →
//!   Mojang-format VersionDetails with `inheritsFrom`.
//!
//! Quilt meta does NOT expose a `stable` boolean on each loader entry.
//! Convention: version strings without a `-` qualifier are stable;
//! `0.23.1` is stable, `0.24.0-beta.1` is not.

use crate::error::{Error, Result};
use crate::network::get_json;
use crate::versions::loaders::LoaderVersion;
use crate::versions::version_json::{parse, VersionDetails};
use serde::Deserialize;

const META_DEFAULT: &str = "https://meta.quiltmc.org";

fn meta_base() -> String {
    std::env::var("FTLAUNCHER_QUILT_META_OVERRIDE")
        .unwrap_or_else(|_| META_DEFAULT.to_string())
}

#[derive(Debug, Deserialize)]
struct RawEntry {
    loader: RawLoader,
}

#[derive(Debug, Deserialize)]
struct RawLoader {
    version: String,
    build: u32,
}

pub(super) async fn list(mc: &str) -> Result<Vec<LoaderVersion>> {
    let url = format!("{}/v3/versions/loader/{mc}", meta_base());
    let raw: Vec<RawEntry> = get_json(&url, "loaders/quilt").await?;
    let mut out: Vec<LoaderVersion> = raw
        .into_iter()
        .map(|e| {
            let stable = !e.loader.version.contains('-');
            LoaderVersion {
                version: e.loader.version,
                stable,
                build: e.loader.build,
            }
        })
        .collect();
    out.sort_by(|a, b| b.build.cmp(&a.build));
    Ok(out)
}

pub(super) async fn profile(mc: &str, ver: &str) -> Result<VersionDetails> {
    let url = format!("{}/v3/versions/loader/{mc}/{ver}/profile/json", meta_base());
    let raw_json: serde_json::Value = get_json(&url, "loaders/quilt").await?;
    let text = serde_json::to_string(&raw_json)
        .map_err(|e| Error::io(url.clone(), format!("serialise: {e}")))?;
    parse(&text).map_err(|e| Error::io(url, format!("parse: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const FIXTURE_LIST: &str = r#"[
      {"loader":{"separator":".","build":1,"maven":"org.quiltmc:quilt-loader:0.23.1","version":"0.23.1"}},
      {"loader":{"separator":".","build":2,"maven":"org.quiltmc:quilt-loader:0.24.0-beta.1","version":"0.24.0-beta.1"}}
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
        // Sorted by build desc.
        assert_eq!(out[0].version, "0.24.0-beta.1");
        assert!(!out[0].stable);
        assert_eq!(out[1].version, "0.23.1");
        assert!(out[1].stable);

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
}

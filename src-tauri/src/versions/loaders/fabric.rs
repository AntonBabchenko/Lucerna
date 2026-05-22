//! Fabric loader meta client.
//!
//! Endpoints:
//! - `https://meta.fabricmc.net/v2/versions/loader/<mc>` →
//!   `[{ loader: { version, build, stable, maven, separator }, intermediary: {...}, launcherMeta: {...} }, …]`
//! - `https://meta.fabricmc.net/v2/versions/loader/<mc>/<loader>/profile/json` →
//!   Mojang-format VersionDetails with `inheritsFrom`.

use crate::error::{Error, Result};
use crate::network::get_json;
use crate::versions::loaders::LoaderVersion;
use crate::versions::version_json::{parse, VersionDetails};
use serde::Deserialize;
use serde::de::DeserializeOwned;

/// Fabric-aware wrapper around `get_json`. Maps 404 (returned by meta
/// for unknown MC + loader combinations) to typed `LoaderUnavailable`
/// so the UI shows "Fabric does not support Minecraft <mc>" instead
/// of raw HTTP 404 text. Non-404 errors fall through.
async fn fetch<T: DeserializeOwned>(url: &str, mc: &str, initiator: &str) -> Result<T> {
    match get_json::<T>(url, initiator).await {
        Ok(v) => Ok(v),
        Err(Error::Network { details, .. }) if details.starts_with("HTTP 404") => {
            Err(Error::LoaderUnavailable {
                loader: "fabric".into(),
                mc_version: mc.into(),
            })
        }
        Err(e) => Err(e),
    }
}

const META_DEFAULT: &str = "https://meta.fabricmc.net";

/// In production, `meta.fabricmc.net`. Tests set
/// `FTLAUNCHER_FABRIC_META_OVERRIDE` to a wiremock URI.
fn meta_base() -> String {
    std::env::var("FTLAUNCHER_FABRIC_META_OVERRIDE")
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
    stable: bool,
}

pub(super) async fn list(mc: &str) -> Result<Vec<LoaderVersion>> {
    let url = format!("{}/v2/versions/loader/{mc}", meta_base());
    // Fabric meta returns `[]` (HTTP 200) for unsupported MC versions —
    // never a 4xx — so empty translates to `LoaderUnavailable` in the
    // caller (`loaders::list_loaders`).
    let raw: Vec<RawEntry> = fetch(&url, mc, "loaders/fabric").await?;
    let mut out: Vec<LoaderVersion> = raw
        .into_iter()
        .map(|e| LoaderVersion {
            version: e.loader.version,
            stable: e.loader.stable,
            build: e.loader.build,
        })
        .collect();
    // Highest build first → UI shows newest at top of dropdown.
    out.sort_by(|a, b| b.build.cmp(&a.build));
    Ok(out)
}

pub(super) async fn profile(mc: &str, ver: &str) -> Result<VersionDetails> {
    let url = format!("{}/v2/versions/loader/{mc}/{ver}/profile/json", meta_base());
    // Fabric meta returns a 404 with a small text body when the
    // combination is invalid. `fetch` maps that to LoaderUnavailable
    // so the UI shows a friendly "Fabric does not support …" instead
    // of raw HTTP 404 text.
    let raw_json: serde_json::Value = fetch(&url, mc, "loaders/fabric").await?;
    let text = serde_json::to_string(&raw_json)
        .map_err(|e| Error::io(url.clone(), format!("serialise: {e}")))?;
    parse(&text).map_err(|e| Error::io(url, format!("parse: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    const FIXTURE_LIST: &str = r#"[
      {"loader":{"separator":".","build":7,"maven":"net.fabricmc:fabric-loader:0.15.7","version":"0.15.7","stable":true},"intermediary":{},"launcherMeta":{}},
      {"loader":{"separator":".","build":6,"maven":"net.fabricmc:fabric-loader:0.15.6","version":"0.15.6","stable":true},"intermediary":{},"launcherMeta":{}},
      {"loader":{"separator":".","build":5,"maven":"net.fabricmc:fabric-loader:0.15.5-beta.1","version":"0.15.5-beta.1","stable":false},"intermediary":{},"launcherMeta":{}}
    ]"#;

    /// Matches real Fabric meta API shape: loader profiles do NOT include
    /// `assetIndex`, `assets`, or `downloads`.
    const FIXTURE_PROFILE: &str = r#"{
      "id": "fabric-loader-0.15.7-1.20.4",
      "inheritsFrom": "1.20.4",
      "type": "release",
      "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
      "libraries": [
        {"name":"net.fabricmc:fabric-loader:0.15.7","url":"https://maven.fabricmc.net/"},
        {"name":"net.fabricmc:sponge-mixin:0.13.4+mixin.0.8.5","url":"https://maven.fabricmc.net/"},
        {"name":"org.ow2.asm:asm:9.6","url":"https://maven.fabricmc.net/"},
        {"name":"net.fabricmc:intermediary:1.20.4","url":"https://maven.fabricmc.net/"}
      ],
      "arguments": {"jvm": [], "game": []}
    }"#;

    #[tokio::test]
    async fn list_against_wiremock_returns_sorted_loader_versions() {
        let _g = test_lock();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/versions/loader/1.20.4"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_LIST))
            .mount(&server)
            .await;

        std::env::set_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        std::env::set_var("FTLAUNCHER_FABRIC_META_OVERRIDE", server.uri());

        let out = list("1.20.4").await.expect("list");
        assert_eq!(out.len(), 3);
        // Sorted by build desc.
        assert_eq!(out[0].build, 7);
        assert_eq!(out[0].version, "0.15.7");
        assert!(out[0].stable);
        assert_eq!(out[2].version, "0.15.5-beta.1");
        assert!(!out[2].stable);

        std::env::remove_var("FTLAUNCHER_FABRIC_META_OVERRIDE");
        std::env::remove_var("FTLAUNCHER_EXTRA_ALLOWED_HOSTS");
    }

    #[test]
    fn profile_fixture_parses_into_version_details() {
        let v = parse(FIXTURE_PROFILE).expect("parse fabric profile");
        assert_eq!(v.id, "fabric-loader-0.15.7-1.20.4");
        assert_eq!(v.inherits_from.as_deref(), Some("1.20.4"));
        assert_eq!(v.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient");
        assert_eq!(v.libraries.len(), 4);
        for lib in &v.libraries {
            assert!(
                lib.downloads.is_none(),
                "fabric profile libs should have no downloads block: {}",
                lib.name
            );
            assert_eq!(lib.url.as_deref(), Some("https://maven.fabricmc.net/"));
        }
        // Real Fabric API does not include these fields — vanilla parent provides them.
        assert!(v.asset_index.is_none(), "loader profile must not have assetIndex");
        assert!(v.assets.is_none(), "loader profile must not have assets");
        assert!(v.downloads.is_none(), "loader profile must not have downloads");
    }
}

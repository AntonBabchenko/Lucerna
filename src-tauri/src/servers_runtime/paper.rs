//! PaperMC Fill v3 build resolution. Pure URL building + JSON mapping; the jar
//! download itself goes through `network::download` with sha256 verification.
//! The legacy api.papermc.io/v2 API is SHUT DOWN (HTTP 410 since 2026-07-01) —
//! never point anything at it. Download URLs are taken verbatim from the API
//! response (fill-data.papermc.io); we never construct object-storage URLs.

use crate::error::{Error, Result};

const BASE_DEFAULT: &str = "https://fill.papermc.io";
/// PaperMC requires a UA identifying the software with a contact URL.
const UA: &str = "AntonBabchenko/Lucerna (github.com/AntonBabchenko/Lucerna)";

pub struct PaperClient {
    base: String,
}

/// A resolved, downloadable server jar for one MC version.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCoreJar {
    /// Build id as a string (Paper: numeric id; kept as string to match
    /// `ServerFile.loader_version`).
    pub build: String,
    /// Lowercase hex digest to verify the download with.
    pub digest: String,
    pub url: String,
}

#[derive(serde::Deserialize)]
struct FillProject {
    /// MC versions grouped by family, e.g. {"1.21": ["1.21.4", ...]}.
    versions: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(serde::Deserialize)]
struct FillBuild {
    id: i64,
    channel: String,
    downloads: std::collections::HashMap<String, FillDownload>,
}

#[derive(serde::Deserialize)]
struct FillDownload {
    #[allow(dead_code)]
    name: String,
    checksums: FillChecksums,
    url: String,
}

#[derive(serde::Deserialize)]
struct FillChecksums {
    sha256: String,
}

impl PaperClient {
    pub fn new() -> Self {
        Self {
            base: BASE_DEFAULT.into(),
        }
    }

    /// Tests inject a wiremock URL here.
    pub fn with_base(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str, mc: &str) -> Result<T> {
        let resp = crate::network::request::get(url, &[("user-agent", UA)], "servers")
            .await
            .map_err(|e| Error::ServerJarUnavailable {
                loader: "paper".into(),
                mc_version: mc.to_string(),
                reason: e.to_string(),
            })?;
        if resp.status == 404 {
            return Err(Error::ServerJarUnavailable {
                loader: "paper".into(),
                mc_version: mc.to_string(),
                reason: "version not available".into(),
            });
        }
        if !(200..300).contains(&resp.status) {
            return Err(Error::ServerJarUnavailable {
                loader: "paper".into(),
                mc_version: mc.to_string(),
                reason: format!("HTTP {}", resp.status),
            });
        }
        serde_json::from_slice(&resp.body).map_err(|e| Error::ServerJarUnavailable {
            loader: "paper".into(),
            mc_version: mc.to_string(),
            reason: format!("decode: {e}"),
        })
    }

    /// Every MC version Paper publishes builds for (family groups flattened;
    /// order not meaningful — callers intersect with the Mojang manifest).
    pub async fn supported_versions(&self) -> Result<Vec<String>> {
        let url = format!("{}/v3/projects/paper", self.base);
        let p: FillProject = self.get_json(&url, "*").await?;
        Ok(p.versions.into_values().flatten().collect())
    }

    /// Newest STABLE-channel build for `mc`. A version with only ALPHA/BETA
    /// builds (fresh MC release) is a typed "no stable build yet" error —
    /// the version stays selectable in the wizard, the copy explains why.
    pub async fn latest_stable_build(&self, mc: &str) -> Result<ResolvedCoreJar> {
        let url = format!("{}/v3/projects/paper/versions/{mc}/builds", self.base);
        let builds: Vec<FillBuild> = self.get_json(&url, mc).await?;
        // Fill returns newest-first; take the first STABLE with a server jar.
        builds
            .into_iter()
            .find(|b| b.channel == "STABLE" && b.downloads.contains_key("server:default"))
            .map(|b| {
                let d = &b.downloads["server:default"];
                ResolvedCoreJar {
                    build: b.id.to_string(),
                    digest: d.checksums.sha256.clone(),
                    url: d.url.clone(),
                }
            })
            .ok_or_else(|| Error::ServerJarUnavailable {
                loader: "paper".into(),
                mc_version: mc.to_string(),
                reason: "no stable Paper build yet for this version".into(),
            })
    }
}

impl Default for PaperClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn latest_stable_build_skips_prerelease_channels() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v3/projects/paper/versions/1.21.4/builds"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[
                  {"id":130,"channel":"ALPHA","downloads":{"server:default":{"name":"paper-1.21.4-130.jar","checksums":{"sha256":"aa"},"url":"https://fill-data.papermc.io/v1/objects/aa/paper-1.21.4-130.jar"}}},
                  {"id":129,"channel":"STABLE","downloads":{"server:default":{"name":"paper-1.21.4-129.jar","checksums":{"sha256":"bb"},"url":"https://fill-data.papermc.io/v1/objects/bb/paper-1.21.4-129.jar"}}}
                ]"#,
            ))
            .mount(&s)
            .await;
        let c = PaperClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let jar = c.latest_stable_build("1.21.4").await.unwrap();
        assert_eq!(jar.build, "129");
        assert_eq!(jar.digest, "bb");
        assert!(jar.url.ends_with("paper-1.21.4-129.jar"));
    }

    #[tokio::test]
    async fn latest_stable_build_errors_when_only_prerelease_exists() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v3/projects/paper/versions/26.3/builds"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{"id":1,"channel":"ALPHA","downloads":{"server:default":{"name":"paper-26.3-1.jar","checksums":{"sha256":"aa"},"url":"https://x/paper.jar"}}}]"#,
            ))
            .mount(&s)
            .await;
        let c = PaperClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let err = c.latest_stable_build("26.3").await.unwrap_err();
        assert!(matches!(
            err,
            crate::error::Error::ServerJarUnavailable { .. }
        ));
    }

    #[tokio::test]
    async fn latest_stable_build_maps_404_to_unavailable() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v3/projects/paper/versions/9.9.9/builds"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let c = PaperClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        assert!(matches!(
            c.latest_stable_build("9.9.9").await.unwrap_err(),
            crate::error::Error::ServerJarUnavailable { .. }
        ));
    }

    #[tokio::test]
    async fn supported_versions_flattens_family_groups() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v3/projects/paper"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"project":{"id":"paper"},"versions":{"1.21":["1.21.4","1.21.3"],"26.1":["26.1.2"]}}"#,
            ))
            .mount(&s)
            .await;
        let c = PaperClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let vs = c.supported_versions().await.unwrap();
        assert!(vs.contains(&"1.21.4".to_string()));
        assert!(vs.contains(&"26.1.2".to_string()));
    }
}

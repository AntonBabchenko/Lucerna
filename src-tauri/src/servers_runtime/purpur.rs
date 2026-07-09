//! PurpurMC v2 build resolution. Community-run infra: check result=="SUCCESS"
//! before trusting a build, verify md5 after download (Purpur publishes
//! nothing stronger — corruption check + HTTPS, documented in the spec).

use crate::error::{Error, Result};
use crate::servers_runtime::paper::ResolvedCoreJar;

const BASE_DEFAULT: &str = "https://api.purpurmc.org";
const UA: &str = "AntonBabchenko/Lucerna (github.com/AntonBabchenko/Lucerna)";
/// How many builds to walk back over failed CI results before giving up.
const MAX_BUILD_PROBES: usize = 5;

pub struct PurpurClient {
    base: String,
}

#[derive(serde::Deserialize)]
struct PurpurProject {
    versions: Vec<String>,
}

#[derive(serde::Deserialize)]
struct PurpurVersion {
    builds: PurpurBuilds,
}

#[derive(serde::Deserialize)]
struct PurpurBuilds {
    #[allow(dead_code)]
    latest: String,
    all: Vec<String>,
}

#[derive(serde::Deserialize)]
struct PurpurBuild {
    build: String,
    #[serde(default)]
    result: String,
    #[serde(default)]
    md5: String,
}

impl PurpurClient {
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
                loader: "purpur".into(),
                mc_version: mc.to_string(),
                reason: e.to_string(),
            })?;
        if resp.status == 404 {
            return Err(Error::ServerJarUnavailable {
                loader: "purpur".into(),
                mc_version: mc.to_string(),
                reason: "version not available".into(),
            });
        }
        if !(200..300).contains(&resp.status) {
            return Err(Error::ServerJarUnavailable {
                loader: "purpur".into(),
                mc_version: mc.to_string(),
                reason: format!("HTTP {}", resp.status),
            });
        }
        serde_json::from_slice(&resp.body).map_err(|e| Error::ServerJarUnavailable {
            loader: "purpur".into(),
            mc_version: mc.to_string(),
            reason: format!("decode: {e}"),
        })
    }

    fn download_url(&self, mc: &str, build: &str) -> String {
        format!("{}/v2/purpur/{mc}/{build}/download", self.base)
    }

    /// Every MC version Purpur publishes builds for, in the order the API
    /// returns them (not sorted — callers intersect with the Mojang manifest).
    pub async fn supported_versions(&self) -> Result<Vec<String>> {
        let url = format!("{}/v2/purpur", self.base);
        let p: PurpurProject = self.get_json(&url, "*").await?;
        Ok(p.versions)
    }

    /// A build is only usable if the API marked it SUCCESS AND published a
    /// non-empty md5 — an empty digest would silently skip download
    /// verification in `network::download`, so it's treated as untrustworthy.
    fn is_usable(build: &PurpurBuild) -> bool {
        build.result == "SUCCESS" && !build.md5.is_empty()
    }

    fn to_resolved(&self, mc: &str, build: &PurpurBuild) -> ResolvedCoreJar {
        ResolvedCoreJar {
            url: self.download_url(mc, &build.build),
            build: build.build.clone(),
            checksum: crate::network::download::Checksum::Md5(build.md5.clone()),
        }
    }

    /// Newest build with result == "SUCCESS" (and a real md5) for `mc`. Tries
    /// `/latest` first; on a failed or unverifiable build walks back over
    /// `builds.all` (newest last, hence `.rev()`), probing at most
    /// MAX_BUILD_PROBES build-info records.
    pub async fn latest_successful_build(&self, mc: &str) -> Result<ResolvedCoreJar> {
        let latest_url = format!("{}/v2/purpur/{mc}/latest", self.base);
        let latest: PurpurBuild = self.get_json(&latest_url, mc).await?;
        if Self::is_usable(&latest) {
            return Ok(self.to_resolved(mc, &latest));
        }
        let ver_url = format!("{}/v2/purpur/{mc}", self.base);
        let ver: PurpurVersion = self.get_json(&ver_url, mc).await?;
        for build in ver
            .builds
            .all
            .iter()
            .rev()
            .filter(|b| **b != latest.build)
            .take(MAX_BUILD_PROBES)
        {
            let info_url = format!("{}/v2/purpur/{mc}/{build}", self.base);
            let info: PurpurBuild = self.get_json(&info_url, mc).await?;
            if Self::is_usable(&info) {
                return Ok(self.to_resolved(mc, &info));
            }
        }
        Err(Error::ServerJarUnavailable {
            loader: "purpur".into(),
            mc_version: mc.to_string(),
            reason: "no successful Purpur build found".into(),
        })
    }
}

impl Default for PurpurClient {
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
    async fn latest_successful_build_happy_path() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/purpur/1.21.4/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"project":"purpur","version":"1.21.4","build":"2321","result":"SUCCESS","md5":"c0ffee"}"#,
            ))
            .mount(&s)
            .await;
        let c = PurpurClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let jar = c.latest_successful_build("1.21.4").await.unwrap();
        assert_eq!(jar.build, "2321");
        assert_eq!(
            jar.checksum,
            crate::network::download::Checksum::Md5("c0ffee".into())
        );
        assert!(jar.url.ends_with("/v2/purpur/1.21.4/2321/download"));
    }

    #[tokio::test]
    async fn latest_successful_build_walks_back_over_failures() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/purpur/1.21.4/latest"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"build":"2321","result":"FAILURE","md5":""}"#),
            )
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/purpur/1.21.4"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"{"builds":{"latest":"2321","all":["2319","2320","2321"]}}"#,
                ),
            )
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/purpur/1.21.4/2320"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"build":"2320","result":"SUCCESS","md5":"beef"}"#),
            )
            .mount(&s)
            .await;
        let c = PurpurClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let jar = c.latest_successful_build("1.21.4").await.unwrap();
        assert_eq!(jar.build, "2320");
        assert_eq!(
            jar.checksum,
            crate::network::download::Checksum::Md5("beef".into())
        );
    }

    #[tokio::test]
    async fn supported_versions_lists_api_order() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/purpur"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"{"project":"purpur","versions":["1.14.1","1.21.4","26.1.2"]}"#,
                ),
            )
            .mount(&s)
            .await;
        let c = PurpurClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        assert_eq!(
            c.supported_versions().await.unwrap(),
            vec!["1.14.1", "1.21.4", "26.1.2"]
        );
    }

    #[tokio::test]
    async fn latest_successful_build_gives_up_when_all_probes_fail() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/purpur/1.21.4/latest"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"build":"2321","result":"FAILURE","md5":""}"#),
            )
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/purpur/1.21.4"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"{"builds":{"latest":"2321","all":["2319","2320","2321"]}}"#,
                ),
            )
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/purpur/1.21.4/2320"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"build":"2320","result":"FAILURE","md5":""}"#),
            )
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/purpur/1.21.4/2319"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"build":"2319","result":"FAILURE","md5":""}"#),
            )
            .mount(&s)
            .await;
        let c = PurpurClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let err = c.latest_successful_build("1.21.4").await.unwrap_err();
        match err {
            crate::error::Error::ServerJarUnavailable { reason, .. } => {
                assert!(reason.contains("no successful"), "reason was: {reason}");
            }
            other => panic!("expected ServerJarUnavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn latest_successful_build_treats_empty_md5_as_untrustworthy() {
        let s = MockServer::start().await;
        // /latest reports SUCCESS but with an empty md5 — must not be trusted
        // (an empty digest would silently skip download verification).
        Mock::given(method("GET"))
            .and(path("/v2/purpur/1.21.4/latest"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"build":"2321","result":"SUCCESS","md5":""}"#),
            )
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/purpur/1.21.4"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"{"builds":{"latest":"2321","all":["2319","2320","2321"]}}"#,
                ),
            )
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/purpur/1.21.4/2320"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"build":"2320","result":"SUCCESS","md5":"beef"}"#),
            )
            .mount(&s)
            .await;
        let c = PurpurClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let jar = c.latest_successful_build("1.21.4").await.unwrap();
        assert_eq!(jar.build, "2320");
        assert_eq!(
            jar.checksum,
            crate::network::download::Checksum::Md5("beef".into())
        );
    }
}

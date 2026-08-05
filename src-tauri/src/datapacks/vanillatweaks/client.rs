//! The two vanillatweaks.net calls. Nothing else in the crate names this
//! host: the listing is a plain GET of a per-family JSON document, and the
//! download is a POST that builds a zip on demand and answers with a link to
//! it.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const BASE_DEFAULT: &str = "https://vanillatweaks.net";

/// One pack as VT describes it. `version` is a plain string like `2.8.21` —
/// there is no version id, and this string is the whole identity of a
/// release. `incompatible` names other packs by their `name`, not their
/// `display`.
#[derive(Debug, Clone, Deserialize, Serialize, specta::Type, PartialEq)]
pub struct VtPack {
    pub name: String,
    pub display: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub incompatible: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type, PartialEq)]
pub struct VtCategory {
    pub category: String,
    pub packs: Vec<VtPack>,
}

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type, PartialEq)]
pub struct VtCatalogue {
    #[serde(rename = "versionName", default)]
    pub version_name: String,
    pub categories: Vec<VtCategory>,
}

pub struct VtClient {
    base: String,
}

impl VtClient {
    pub fn new() -> Self {
        Self {
            base: BASE_DEFAULT.into(),
        }
    }

    /// Tests inject a wiremock URL here.
    pub fn with_base(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }

    /// The pack listing for one family. A 404 means VT publishes nothing for
    /// that Minecraft family at all — a typed answer about the upstream
    /// catalogue, not a transport failure.
    pub async fn catalogue(&self, family: &str) -> Result<VtCatalogue> {
        let url = format!(
            "{}/assets/resources/json/{family}/dpcategories.json",
            self.base
        );
        let res = crate::network::request::get(&url, &[], "vt-catalogue").await?;
        if res.status == 404 {
            return Err(Error::VanillaTweaksUnavailable {
                mc_version: family.to_string(),
            });
        }
        if !(200..300).contains(&res.status) {
            return Err(Error::VanillaTweaksBuildFailed {
                message: format!("the pack list request failed with HTTP {}", res.status),
            });
        }
        serde_json::from_slice(&res.body).map_err(|e| Error::VanillaTweaksBuildFailed {
            message: format!("the pack list was not the expected JSON: {e}"),
        })
    }

    /// Ask VT to build a bundle for `selection` and return the absolute URL
    /// of the zip it produced. `selection` is `(category, [pack name])` — the
    /// endpoint is keyed by category, which is why a pack's identity in our
    /// registry has to carry its category too.
    pub async fn build_link(
        &self,
        family: &str,
        selection: &[(String, Vec<String>)],
    ) -> Result<String> {
        // BTreeMap so the encoded body is deterministic — a stable body makes
        // the request diffable in a log and the test's matcher meaningful.
        let by_category: std::collections::BTreeMap<&str, &Vec<String>> = selection
            .iter()
            .map(|(cat, names)| (cat.as_str(), names))
            .collect();
        let packs_json =
            serde_json::to_string(&by_category).map_err(|e| Error::VanillaTweaksBuildFailed {
                message: format!("could not encode the selection: {e}"),
            })?;
        let body = format!(
            "packs={}&version={}",
            urlencoding::encode(&packs_json),
            urlencoding::encode(family)
        );
        let url = format!("{}/assets/server/zipdatapacks.php", self.base);
        let res = crate::network::request::post(
            &url,
            &[("content-type", "application/x-www-form-urlencoded")],
            body.as_bytes(),
            "vt-build",
        )
        .await?;
        if !(200..300).contains(&res.status) {
            return Err(Error::VanillaTweaksBuildFailed {
                message: format!("the build request failed with HTTP {}", res.status),
            });
        }

        #[derive(Deserialize)]
        struct BuildResponse {
            #[serde(default)]
            status: String,
            #[serde(default)]
            link: String,
            #[serde(default)]
            message: String,
        }

        let parsed: BuildResponse =
            serde_json::from_slice(&res.body).map_err(|e| Error::VanillaTweaksBuildFailed {
                message: format!("the build response was not the expected JSON: {e}"),
            })?;
        if parsed.status != "success" {
            return Err(Error::VanillaTweaksBuildFailed {
                message: parsed.message,
            });
        }
        Ok(format!("{}{}", self.base, parsed.link))
    }
}

impl Default for VtClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SAMPLE: &str = r#"{
      "versionName": "1.21",
      "categories": [
        { "category": "survival",
          "packs": [
            {"name":"armor statues","display":"Armor Statues","version":"2.8.21",
             "description":"Alter armor stands.","incompatible":[],"lastupdated":1524346225},
            {"name":"graves","display":"Graves","version":"2.8.5",
             "description":"Keeps your stuff.","incompatible":["armor statues"],
             "lastupdated":1624346225}
          ] }
      ] }"#;

    #[tokio::test]
    async fn catalogue_parses_categories_and_packs() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/assets/resources/json/1.21/dpcategories.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(SAMPLE))
            .mount(&s)
            .await;

        let c = VtClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let cat = c.catalogue("1.21").await.unwrap();

        assert_eq!(cat.categories.len(), 1);
        assert_eq!(cat.categories[0].category, "survival");
        assert_eq!(cat.categories[0].packs.len(), 2);
        assert_eq!(cat.categories[0].packs[1].name, "graves");
        assert_eq!(cat.categories[0].packs[1].version, "2.8.5");
        assert_eq!(
            cat.categories[0].packs[1].incompatible,
            vec!["armor statues".to_string()]
        );
    }

    #[tokio::test]
    async fn a_missing_family_becomes_the_typed_unavailable_error() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/assets/resources/json/9.99/dpcategories.json"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;

        let c = VtClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let err = c.catalogue("9.99").await.unwrap_err();
        assert!(
            matches!(err, Error::VanillaTweaksUnavailable { .. }),
            "expected VanillaTweaksUnavailable, got {err:?}"
        );
    }

    #[tokio::test]
    async fn build_posts_packs_grouped_by_category_and_returns_an_absolute_link() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/assets/server/zipdatapacks.php"))
            .and(body_string_contains("version=1.21"))
            // The selection is a JSON object keyed by category. Pack names
            // carry spaces, so the encoding of the space is load-bearing.
            .and(body_string_contains("survival"))
            .and(body_string_contains("armor%20statues"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"status":"success","link":"/assets/dl/1234/datapacks.zip","message":""}"#,
            ))
            .mount(&s)
            .await;

        let c = VtClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let link = c
            .build_link(
                "1.21",
                &[("survival".into(), vec!["armor statues".into()])],
            )
            .await
            .unwrap();
        assert_eq!(link, format!("{}/assets/dl/1234/datapacks.zip", s.uri()));
    }

    #[tokio::test]
    async fn a_refused_build_carries_the_servers_own_message() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/assets/server/zipdatapacks.php"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"status":"error","link":"","message":"no packs selected"}"#,
            ))
            .mount(&s)
            .await;

        let c = VtClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let err = c.build_link("1.21", &[]).await.unwrap_err();
        match err {
            Error::VanillaTweaksBuildFailed { message } => {
                assert_eq!(message, "no packs selected");
            }
            other => panic!("expected VanillaTweaksBuildFailed, got {other:?}"),
        }
    }
}

//! `ModPlatform` for Vanilla Tweaks — real where it can be, loudly
//! unsupported everywhere else.
//!
//! Only `datapack_versions` is implemented, and that is the whole point: both
//! update checks reach a source exclusively through this trait, so
//! implementing this one method makes VT rows update-checkable with no change
//! to either caller. Routing VT to `UnsupportedModPlatform` instead would be
//! silently wrong — that type does not override `datapack_versions`, and the
//! trait's default answers `Ok(vec![])`, which `classify_asset_update` reads
//! as "up to date" forever.

use async_trait::async_trait;

use crate::datapacks::vanillatweaks::{family_for, VtClient};
use crate::error::Error;
use crate::mods::platform::{
    LoaderKind, ModFile, ModPlatform, ModProject, ModSearchPage, ModSearchQuery, ModSource,
    ModVersion, ResolvedDeps,
};

pub struct VanillaTweaksPlatform {
    /// `None` in production; tests inject a wiremock base here.
    base: Option<String>,
}

impl VanillaTweaksPlatform {
    pub fn new() -> Self {
        Self { base: None }
    }

    pub fn with_base(base: impl Into<String>) -> Self {
        Self {
            base: Some(base.into()),
        }
    }

    fn client(&self) -> VtClient {
        match &self.base {
            Some(b) => VtClient::with_base(b.clone()),
            None => VtClient::new(),
        }
    }

    fn unsupported(&self) -> Error {
        Error::ModsPlatformUnsupported {
            platform: ModSource::VanillaTweaks,
        }
    }
}

impl Default for VanillaTweaksPlatform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ModPlatform for VanillaTweaksPlatform {
    async fn search(&self, _q: &ModSearchQuery) -> Result<ModSearchPage, Error> {
        Err(self.unsupported())
    }

    async fn project(&self, _project_id: &str) -> Result<ModProject, Error> {
        Err(self.unsupported())
    }

    async fn versions(
        &self,
        _project_id: &str,
        _mc_version: Option<&str>,
        _loader: Option<LoaderKind>,
    ) -> Result<Vec<ModVersion>, Error> {
        Err(self.unsupported())
    }

    async fn resolve_deps(
        &self,
        _version: &ModVersion,
        _mc_version: &str,
        _loader: LoaderKind,
    ) -> Result<ResolvedDeps, Error> {
        Err(self.unsupported())
    }

    async fn plugin_versions(
        &self,
        _project_id: &str,
        _mc_version: Option<&str>,
        _plugin_loaders: &[&str],
    ) -> Result<Vec<ModVersion>, Error> {
        Err(self.unsupported())
    }

    /// `project_id` is `<category>/<name>` — the category is part of a VT
    /// pack's identity because the build request is keyed by it.
    ///
    /// Answers at most one version: VT publishes exactly one release of a
    /// pack per Minecraft family, and its `version` string is that release's
    /// whole identity.
    async fn datapack_versions(
        &self,
        project_id: &str,
        mc_version: Option<&str>,
    ) -> Result<Vec<ModVersion>, Error> {
        let mc = mc_version.unwrap_or_default();
        let family = family_for(mc).ok_or_else(|| Error::VanillaTweaksUnavailable {
            mc_version: mc.to_string(),
        })?;
        let (category, name) =
            project_id
                .split_once('/')
                .ok_or_else(|| Error::VanillaTweaksBuildFailed {
                    message: format!("'{project_id}' is not a <category>/<name> pack id"),
                })?;
        let cat = self.client().catalogue(&family).await?;
        let Some(pack) = cat
            .categories
            .iter()
            .filter(|c| c.category.eq_ignore_ascii_case(category))
            .flat_map(|c| c.packs.iter())
            .find(|p| p.name.eq_ignore_ascii_case(name))
        else {
            // The pack is gone from this family. Not an error — an honest
            // "nothing newer", which reads as up to date.
            return Ok(Vec::new());
        };
        Ok(vec![ModVersion {
            source: ModSource::VanillaTweaks,
            project_id: project_id.to_string(),
            version_id: pack.version.clone(),
            name: pack.display.clone(),
            version_number: pack.version.clone(),
            mc_versions: vec![family.clone()],
            loaders: Vec::new(),
            primary_file: ModFile {
                // Predicted for display only. The authoritative name comes
                // back inside the built bundle — see `unpack::split_bundle`.
                filename: format!("{} v{}.zip", pack.name, pack.version),
                // Deliberately empty: a VT pack has no direct URL, its bytes
                // exist only after a build request. Any code that tries to
                // fetch this fails loudly instead of downloading something
                // plausible-looking.
                url: String::new(),
                sha1: None,
                size: 0.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: Vec::new(),
            published_at: None,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::ModPlatform;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SAMPLE: &str = r#"{"versionName":"1.21","categories":[
      {"category":"survival","packs":[
        {"name":"graves","display":"Graves","version":"2.8.5",
         "description":"","incompatible":[],"lastupdated":1}]}]}"#;

    async fn server() -> MockServer {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/assets/resources/json/1.21/dpcategories.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(SAMPLE))
            .mount(&s)
            .await;
        s
    }

    #[tokio::test]
    async fn datapack_versions_answers_one_synthetic_version_for_a_known_pack() {
        let s = server().await;
        let p = VanillaTweaksPlatform::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let vs = p
            .datapack_versions("survival/graves", Some("1.21.4"))
            .await
            .unwrap();
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].version_id, "2.8.5");
        assert_eq!(vs[0].project_id, "survival/graves");
        assert_eq!(vs[0].source, ModSource::VanillaTweaks);
        // There is no direct URL for a VT pack: the bytes only exist after a
        // build request. Anything that tries to fetch this must fail loudly
        // rather than download something plausible-looking.
        assert_eq!(vs[0].primary_file.url, "");
    }

    #[tokio::test]
    async fn an_unknown_pack_answers_an_empty_list_not_an_error() {
        let s = server().await;
        let p = VanillaTweaksPlatform::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let vs = p
            .datapack_versions("survival/gone", Some("1.21.4"))
            .await
            .unwrap();
        assert!(vs.is_empty());
    }

    #[tokio::test]
    async fn a_version_with_no_family_is_the_typed_unavailable_error() {
        // No request is made at all, so no mock server and no seam are needed.
        let p = VanillaTweaksPlatform::with_base("http://127.0.0.1:1");
        let err = p
            .datapack_versions("survival/graves", Some("1.12.2"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, crate::error::Error::VanillaTweaksUnavailable { .. }),
            "expected VanillaTweaksUnavailable, got {err:?}"
        );
    }

    #[tokio::test]
    async fn searching_vanilla_tweaks_is_refused_rather_than_answered_emptily() {
        let p = VanillaTweaksPlatform::new();
        let err = p
            .versions("survival/graves", Some("1.21.4"), None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, crate::error::Error::ModsPlatformUnsupported { .. }),
            "expected ModsPlatformUnsupported, got {err:?}"
        );
    }
}

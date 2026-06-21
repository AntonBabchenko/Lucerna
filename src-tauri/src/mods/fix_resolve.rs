//! Pinned resolution of a curated fix mod to a downloadable version. Unlike
//! `cited_resolve` (fuzzy name/slug search), this NEVER searches: it takes an
//! exact `(project_id, version_id)` and confirms that version is published,
//! compatible with the instance mc+loader, integrity-verifiable (sha1 present),
//! and redistributable. Returns `None` to DEGRADE (the UI then offers a browser
//! link instead of a one-click install) — it never guesses a substitute.

use crate::mods::platform::{
    drop_filename_loader_mismatches, LoaderKind, ModPlatform, ModSource, VersionRef,
};

/// A successfully resolved pinned fix version.
#[derive(Debug, Clone)]
pub struct PinnedFix {
    pub target: VersionRef,
    pub version_label: String,
}

/// Resolve an exact pinned version, or `None` to degrade. `versions()` already
/// facets by mc+loader, so any returned version is compatible; we additionally
/// require the pinned `version_id` to be present, integrity-verifiable, and
/// redistributable.
pub async fn resolve_pinned(
    source: ModSource,
    project_id: &str,
    version_id: &str,
    mc_version: &str,
    loader: LoaderKind,
    plat: &dyn ModPlatform,
) -> Option<PinnedFix> {
    let versions = plat
        .versions(project_id, Some(mc_version), Some(loader))
        .await
        .ok()?;
    let versions = drop_filename_loader_mismatches(versions, Some(loader));
    let v = versions.iter().find(|v| v.version_id == version_id)?;
    // No-TOFU: refuse a file with no sha1 to verify against.
    let sha1_ok = v
        .primary_file
        .sha1
        .as_deref()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if !sha1_ok || !v.primary_file.distribution_allowed {
        return None;
    }
    Some(PinnedFix {
        target: VersionRef {
            source,
            project_id: v.project_id.clone(),
            version_id: v.version_id.clone(),
        },
        version_label: v.version_number.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::modrinth::ModrinthClient;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Build a Modrinth `/v2/project/{id}/version` response body.
    ///
    /// `sha1`: the sha1 hash string inside `hashes` — pass `""` to exercise the
    /// empty-sha1 degrade path (parsed as `Some("")`, rejected by `!s.is_empty()`).
    ///
    /// `include_file`: when `false`, the `files` array is empty. The Modrinth
    /// client converts an empty `files` array to the fallback `ModFile` with
    /// `sha1: None` and `distribution_allowed: false` — exercising the
    /// distribution-not-allowed / no-file degrade path.
    fn versions_body(version_id: &str, sha1: &str, include_file: bool) -> String {
        let files = if include_file {
            format!(
                r#"[{{
                    "url": "https://cdn.modrinth.com/fix.jar",
                    "filename": "tfmg-lag-fix-1.20.1-forge-1.0.3.jar",
                    "hashes": {{"sha1": "{sha1}"}},
                    "size": 2048,
                    "primary": true
                }}]"#
            )
        } else {
            "[]".to_string()
        };
        format!(
            r#"[{{
                "id": "{version_id}",
                "project_id": "p1",
                "name": "Fix 1.0.3",
                "version_number": "1.0.3",
                "game_versions": ["1.20.1"],
                "loaders": ["forge"],
                "date_published": "2026-05-01T00:00:00Z",
                "files": {files},
                "dependencies": []
            }}]"#
        )
    }

    async fn server_returning(body: String) -> MockServer {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/p1/version"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&s)
            .await;
        s
    }

    #[tokio::test]
    async fn compatible_pinned_version_resolves() {
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let s = server_returning(versions_body("v-pin", "deadbeef", true)).await;
        let mr = ModrinthClient::with_base(s.uri());
        let out =
            resolve_pinned(ModSource::Modrinth, "p1", "v-pin", "1.20.1", LoaderKind::Forge, &mr)
                .await;
        let pinned = out.expect("pinned version resolves");
        assert_eq!(pinned.target.version_id, "v-pin");
        assert_eq!(pinned.version_label, "1.0.3");
    }

    #[tokio::test]
    async fn pinned_id_absent_degrades() {
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // Modrinth returns a different version_id — the pinned id is not present.
        let s = server_returning(versions_body("v-other", "deadbeef", true)).await;
        let mr = ModrinthClient::with_base(s.uri());
        let out =
            resolve_pinned(ModSource::Modrinth, "p1", "v-pin", "1.20.1", LoaderKind::Forge, &mr)
                .await;
        assert!(out.is_none(), "absent pinned id must degrade");
    }

    #[tokio::test]
    async fn distribution_disabled_degrades() {
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // Empty `files` array → Modrinth client sets distribution_allowed=false
        // and sha1=None on the fallback ModFile. Both checks in resolve_pinned
        // fail, so the call degrades to None. This exercises the
        // distribution_allowed=false path (the Modrinth JSON format has no
        // explicit distribution flag; absence of a file IS the signal).
        let s = server_returning(versions_body("v-pin", "", false)).await;
        let mr = ModrinthClient::with_base(s.uri());
        let out =
            resolve_pinned(ModSource::Modrinth, "p1", "v-pin", "1.20.1", LoaderKind::Forge, &mr)
                .await;
        assert!(out.is_none(), "distribution_allowed=false must degrade");
    }

    #[tokio::test]
    async fn missing_sha1_degrades() {
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // Empty sha1 string → parsed as Some("") → rejected by !s.is_empty() check.
        // distribution_allowed is true (file is present), so this exercises the
        // sha1-only degrade path.
        let s = server_returning(versions_body("v-pin", "", true)).await;
        let mr = ModrinthClient::with_base(s.uri());
        let out =
            resolve_pinned(ModSource::Modrinth, "p1", "v-pin", "1.20.1", LoaderKind::Forge, &mr)
                .await;
        assert!(out.is_none(), "empty sha1 (no-TOFU) must degrade");
    }

    #[tokio::test]
    async fn transport_error_degrades() {
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // No mock mounted — any request to the mock server returns 404/connection
        // error. The .ok()? on plat.versions() degrades gracefully to None.
        let s = MockServer::start().await;
        let mr = ModrinthClient::with_base(s.uri());
        let out =
            resolve_pinned(ModSource::Modrinth, "p1", "v-pin", "1.20.1", LoaderKind::Forge, &mr)
                .await;
        assert!(out.is_none(), "platform error must degrade, not panic");
    }
}

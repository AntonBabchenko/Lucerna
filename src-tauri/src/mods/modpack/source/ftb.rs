use async_trait::async_trait;
use tauri::Manager;

use crate::error::Error;
use crate::mods::modpack::schema::{
    ModpackProject, ModpackSearchPage, ModpackSort, ModpackVersionEntry,
};
use crate::mods::modpack::source::{ModpackSource, SourceCaps};
use crate::mods::platform::{GalleryImage, LoaderKind, ModSource};

pub struct FtbModpackSource;

const FTB_BASE: &str = "https://api.modpacks.ch";

// ── helpers ───────────────────────────────────────────────────────────────────

/// Map an FTB modloader target name to a `LoaderKind`, if recognised.
fn loader_kind_from_name(name: &str) -> Option<LoaderKind> {
    match name {
        "forge" => Some(LoaderKind::Forge),
        "fabric" => Some(LoaderKind::Fabric),
        "neoforge" => Some(LoaderKind::NeoForge),
        "quilt" => Some(LoaderKind::Quilt),
        _ => None,
    }
}

/// Find the minecraft-game target version for a slice of `FtbTarget`s.
fn mc_version_from_targets(targets: &[crate::mods::modpack::ftb_api::FtbTarget]) -> Option<String> {
    targets
        .iter()
        .find(|t| t.target_type == "game" && t.name == "minecraft")
        .map(|t| t.version.clone())
}

/// Search FTB modpacks, page, filter, and return a `ModpackSearchPage`.
///
/// FTB search returns ALL matching ids with no server-side offset paging;
/// filters are applied client-side after fetching pack details.
pub(crate) async fn search_impl(
    base: &str,
    query: &str,
    page: u32,
    mc: Option<&str>,
    loader: Option<LoaderKind>,
    page_size: u32,
) -> Result<ModpackSearchPage, Error> {
    use crate::mods::modpack::ftb_api;

    // FTB search has no server-side offset paging; we slice the id list client-side.
    let all_ids = ftb_api::search_ids(base, query, page_size).await?;
    let page_ids: Vec<u64> = all_ids
        .into_iter()
        .skip((page * page_size) as usize)
        .take(page_size as usize)
        .collect();

    // N+1 bounded by page_size: fetch detail for each id in the page slice
    // sequentially. A sequential loop is acceptable and avoids the `futures` crate.
    let mut hits = Vec::with_capacity(page_ids.len());
    for id in page_ids {
        match ftb_api::pack_detail(base, id).await {
            Err(e) => {
                // One bad pack shouldn't kill the page — skip and log.
                eprintln!("[ftb] skipping pack {id}: {e}");
                continue;
            }
            Ok(detail) => {
                // icon_url: prefer art_type == "square" (FTB icon), else first art.
                let icon_url = detail
                    .art
                    .iter()
                    .find(|a| a.art_type == "square")
                    .or_else(|| detail.art.first())
                    .map(|a| a.url.clone());

                // latest_mc_version: from the NEWEST version (max by updated).
                let latest_mc_version = detail
                    .versions
                    .iter()
                    .max_by_key(|v| v.updated)
                    .and_then(|v| mc_version_from_targets(&v.targets));

                // supported_loaders: union of modloader target names across all
                // versions, mapped to LoaderKind, deduped.
                let mut supported_loaders: Vec<LoaderKind> = Vec::new();
                for ver in &detail.versions {
                    for t in &ver.targets {
                        if t.target_type == "modloader" {
                            if let Some(kind) = loader_kind_from_name(&t.name) {
                                if !supported_loaders.contains(&kind) {
                                    supported_loaders.push(kind);
                                }
                            }
                        }
                    }
                }

                hits.push(crate::mods::modpack::schema::ModpackHit {
                    project_id: id.to_string(),
                    slug: id.to_string(),
                    title: detail.name,
                    description: detail.synopsis,
                    icon_url,
                    downloads: detail.installs as f64,
                    latest_mc_version,
                    supported_loaders,
                    source: ModSource::Ftb,
                    distribution_allowed: None,
                });
            }
        }
    }

    // caps.supports_server_filter=false — FTB filters are best-effort client-side.
    if let Some(mc_ver) = mc {
        hits.retain(|h| {
            h.latest_mc_version
                .as_deref()
                .map(|v| v == mc_ver)
                .unwrap_or(false)
        });
    }
    if let Some(want_loader) = loader {
        hits.retain(|h| h.supported_loaders.contains(&want_loader));
    }

    // total is approximate (post-filter count; actual total from FTB is not
    // available after client-side filtering).
    let total = hits.len() as u32;
    Ok(ModpackSearchPage {
        hits,
        total,
        offset: page * page_size,
        limit: page_size,
    })
}

/// Return a version list for the given FTB pack id.
pub(crate) async fn get_versions_impl(
    base: &str,
    project_id: &str,
) -> Result<Vec<ModpackVersionEntry>, Error> {
    use crate::mods::modpack::ftb_api;

    let id: u64 = project_id
        .parse()
        .map_err(|_| Error::ModpackManifestInvalid {
            format: "ftb".into(),
            details: "non-numeric pack id".into(),
        })?;
    let detail = ftb_api::pack_detail(base, id).await?;

    let entries = detail
        .versions
        .iter()
        .map(|v| {
            let game_versions: Vec<String> = v
                .targets
                .iter()
                .filter(|t| t.target_type == "game" && t.name == "minecraft")
                .map(|t| t.version.clone())
                .collect();

            let loaders: Vec<String> = v
                .targets
                .iter()
                .filter(|t| t.target_type == "modloader")
                .map(|t| t.name.clone())
                .collect();

            let date_published = chrono::DateTime::from_timestamp(v.updated, 0)
                .map(|d| d.to_rfc3339())
                .unwrap_or_default();

            ModpackVersionEntry {
                id: v.id.to_string(),
                name: v.name.clone(),
                version_number: v.name.clone(),
                game_versions,
                loaders,
                date_published,
            }
        })
        .collect();

    Ok(entries)
}

/// Return project detail (body HTML + gallery) for the given FTB pack id.
pub(crate) async fn get_project_impl(
    base: &str,
    project_id: &str,
) -> Result<ModpackProject, Error> {
    use crate::mods::modpack::ftb_api;
    use crate::mods::render;

    let id: u64 = project_id
        .parse()
        .map_err(|_| Error::ModpackManifestInvalid {
            format: "ftb".into(),
            details: "non-numeric pack id".into(),
        })?;
    let detail = ftb_api::pack_detail(base, id).await?;

    let md_src = if !detail.description.is_empty() {
        &detail.description
    } else {
        &detail.synopsis
    };
    let body_html = render::markdown_to_safe_html(md_src);

    let gallery: Vec<GalleryImage> = detail
        .art
        .iter()
        .filter(|a| render::is_safe_image_url(&a.url))
        .map(|a| GalleryImage {
            url: a.url.clone(),
            title: None,
        })
        .collect();

    Ok(ModpackProject {
        body_html,
        gallery,
        website_url: None,
    })
}

/// Download the FTB version manifest, map it to `ModpackSummary`, serialise to
/// a `.ftbpack.json` sidecar in the OS temp dir, and return the path.
pub(crate) async fn stage_impl(
    app: &tauri::AppHandle,
    base: &str,
    project_id: &str,
    version_id: &str,
) -> Result<String, Error> {
    use crate::mods::modpack::{ftb_api, ftb_map};

    let id: u64 = project_id
        .parse()
        .map_err(|_| Error::ModpackManifestInvalid {
            format: "ftb".into(),
            details: "non-numeric pack id".into(),
        })?;
    let vid: u64 = version_id
        .parse()
        .map_err(|_| Error::ModpackManifestInvalid {
            format: "ftb".into(),
            details: "non-numeric version id".into(),
        })?;

    let detail = ftb_api::pack_detail(base, id).await?;

    // Find the version ref to get its display name; fall back to version_id string.
    let version_name = detail
        .versions
        .iter()
        .find(|v| v.id == vid)
        .map(|v| v.name.clone())
        .unwrap_or_else(|| version_id.to_string());

    let manifest = ftb_api::version_manifest(base, id, vid).await?;
    let summary = ftb_map::map_version(&detail.name, &version_name, &manifest);

    // Serialise ModpackSummary and write to temp sidecar.
    let bytes = serde_json::to_vec(&summary).map_err(|e| Error::ModsDecode {
        platform: "ftb".into(),
        details: e.to_string(),
    })?;

    // Mirror the temp-dir pattern from source/stage.rs::write_to_temp.
    let temp_dir = app
        .path()
        .temp_dir()
        .map_err(|e| Error::Io {
            path: "<temp>".into(),
            details: e.to_string(),
        })?
        .join("lucerna")
        .join("modpack");
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| Error::Io {
            path: temp_dir.display().to_string(),
            details: e.to_string(),
        })?;
    // Filename MUST end with .ftbpack.json (Task 9 detects FTB by this extension).
    let dest = temp_dir.join(format!("{}.ftbpack.json", uuid::Uuid::new_v4()));
    tokio::fs::write(&dest, &bytes)
        .await
        .map_err(|e| Error::Io {
            path: dest.display().to_string(),
            details: e.to_string(),
        })?;

    Ok(dest.to_string_lossy().to_string())
}

// ── trait impl ────────────────────────────────────────────────────────────────

#[async_trait]
impl ModpackSource for FtbModpackSource {
    fn caps(&self) -> SourceCaps {
        SourceCaps {
            needs_api_key: false,
            supports_server_filter: false,
            can_export: false,
        }
    }

    async fn search(
        &self,
        query: &str,
        page: u32,
        mc_version: Option<&str>,
        loader: Option<LoaderKind>,
        _sort: ModpackSort,
        page_size: u32,
    ) -> Result<ModpackSearchPage, Error> {
        search_impl(FTB_BASE, query, page, mc_version, loader, page_size).await
    }

    async fn get_versions(&self, project_id: &str) -> Result<Vec<ModpackVersionEntry>, Error> {
        get_versions_impl(FTB_BASE, project_id).await
    }

    async fn get_project(&self, project_id: &str) -> Result<ModpackProject, Error> {
        get_project_impl(FTB_BASE, project_id).await
    }

    async fn stage_version_to_temp(
        &self,
        app: &tauri::AppHandle,
        project_id: &str,
        version_id: &str,
    ) -> Result<String, Error> {
        stage_impl(app, FTB_BASE, project_id, version_id).await
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::ModSource;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    fn pack_detail_json(id: u64, mc: &str, loader: &str, installs: u64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "name": format!("Pack {id}"),
            "synopsis": format!("Synopsis for pack {id}"),
            "description": format!("Full description for pack {id}"),
            "art": [
                {
                    "url": format!("https://dist.modpacks.ch/packs/{id}/art/square.png"),
                    "type": "square"
                },
                {
                    "url": format!("https://dist.modpacks.ch/packs/{id}/art/wide.png"),
                    "type": "wide"
                }
            ],
            "authors": [{ "name": "Test Author" }],
            "installs": installs,
            "versions": [
                {
                    "id": 6495u64,
                    "name": "1.7.0",
                    "type": "Release",
                    "targets": [
                        {
                            "version": "36.2.39",
                            "id": 736u64,
                            "name": loader,
                            "type": "modloader",
                            "updated": 1700000002i64
                        },
                        {
                            "version": mc,
                            "id": 100u64,
                            "name": "minecraft",
                            "type": "game",
                            "updated": 1700000003i64
                        }
                    ],
                    "updated": 1700000010i64
                }
            ]
        })
    }

    /// Search fetches pack detail for each id, builds a ModpackHit per
    /// detail, and tags hits with source=Ftb.
    #[tokio::test]
    async fn ftb_search_fetches_detail_per_id() {
        let _g = test_lock();
        let s = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/public/modpack/search/20"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "packs": [91u64], "curseforge": [], "total": 1 }),
            ))
            .mount(&s)
            .await;

        Mock::given(method("GET"))
            .and(path("/public/modpack/91"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(pack_detail_json(91, "1.16.5", "forge", 500_000)),
            )
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let page = search_impl(&s.uri(), "test", 0, None, None, 20)
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        assert_eq!(page.hits.len(), 1);
        let hit = &page.hits[0];
        assert_eq!(hit.source, ModSource::Ftb);
        assert_eq!(hit.project_id, "91");
        assert_eq!(hit.title, "Pack 91");
        assert_eq!(hit.latest_mc_version, Some("1.16.5".to_string()));
        assert!(
            hit.supported_loaders.contains(&LoaderKind::Forge),
            "expected Forge in supported_loaders, got {:?}",
            hit.supported_loaders
        );
        // square art is preferred as icon
        assert!(
            hit.icon_url.as_deref().unwrap_or("").contains("square.png"),
            "expected square art URL, got {:?}",
            hit.icon_url
        );
    }

    /// Client-side MC version filter: two packs targeting different MC
    /// versions — only the matching one is returned when mc is set.
    #[tokio::test]
    async fn ftb_search_filters_mc_client_side() {
        let _g = test_lock();
        let s = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/public/modpack/search/20"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "packs": [91u64, 92u64], "curseforge": [], "total": 2 }),
            ))
            .mount(&s)
            .await;

        Mock::given(method("GET"))
            .and(path("/public/modpack/91"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(pack_detail_json(91, "1.16.5", "forge", 100)),
            )
            .mount(&s)
            .await;

        Mock::given(method("GET"))
            .and(path("/public/modpack/92"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(pack_detail_json(92, "1.20.1", "fabric", 200)),
            )
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let page = search_impl(&s.uri(), "test", 0, Some("1.20.1"), None, 20)
            .await
            .unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        assert_eq!(
            page.hits.len(),
            1,
            "only the 1.20.1 pack should survive the client-side filter"
        );
        assert_eq!(page.hits[0].project_id, "92");
    }

    /// get_versions maps FtbVersionRef fields into ModpackVersionEntry correctly.
    #[tokio::test]
    async fn ftb_get_versions_maps_fields() {
        let _g = test_lock();
        let s = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/public/modpack/91"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(pack_detail_json(91, "1.16.5", "forge", 500_000)),
            )
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let versions = get_versions_impl(&s.uri(), "91").await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        assert_eq!(versions.len(), 1);
        let v = &versions[0];
        assert_eq!(v.id, "6495");
        assert_eq!(v.name, "1.7.0");
        assert_eq!(v.version_number, "1.7.0");
        assert_eq!(v.game_versions, vec!["1.16.5"]);
        assert_eq!(v.loaders, vec!["forge"]);
        assert!(!v.date_published.is_empty(), "date_published must be set");
    }

    /// get_project maps body_html from description + gallery from art.
    #[tokio::test]
    async fn ftb_get_project_maps_detail() {
        let _g = test_lock();
        let s = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/public/modpack/91"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(pack_detail_json(91, "1.16.5", "forge", 500_000)),
            )
            .mount(&s)
            .await;

        std::env::set_var("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost");
        let proj = get_project_impl(&s.uri(), "91").await.unwrap();
        std::env::remove_var("LUCERNA_EXTRA_ALLOWED_HOSTS");

        assert!(!proj.body_html.is_empty(), "body_html must be non-empty");
        // Both art entries are https:// — both should appear in gallery.
        assert_eq!(
            proj.gallery.len(),
            2,
            "expected 2 gallery images (both https art entries)"
        );
        assert!(proj.gallery.iter().all(|g| g.url.starts_with("https://")));
    }

    /// Non-numeric project_id returns ModpackManifestInvalid.
    #[tokio::test]
    async fn ftb_get_versions_non_numeric_id_errors() {
        let _g = test_lock();
        let result = get_versions_impl("http://unused", "not-a-number").await;
        assert!(
            matches!(result, Err(Error::ModpackManifestInvalid { .. })),
            "expected ModpackManifestInvalid, got: {result:?}"
        );
    }
}

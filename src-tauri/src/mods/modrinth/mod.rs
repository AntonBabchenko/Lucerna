//! Modrinth v2 API client.

mod types;

use async_trait::async_trait;

use crate::error::Error;
use crate::mods::platform::*;
use std::collections::HashMap;

const BASE_DEFAULT: &str = "https://api.modrinth.com";
const UA: &str = "AntonBabchenko/Lucerna (github.com/AntonBabchenko/Lucerna)";

pub struct ModrinthClient {
    base: String,
}

/// The full identity of a Modrinth version matched by file hash.
#[derive(Debug, Clone)]
pub struct HashVersion {
    pub project_id: String,
    pub version_id: String,
    pub version_number: String,
    pub name: String,
}

impl ModrinthClient {
    pub fn new() -> Self {
        Self {
            base: BASE_DEFAULT.into(),
        }
    }

    /// Tests inject a wiremock URL here.
    pub fn with_base(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }

    fn loader_facet(loader: LoaderKind) -> &'static str {
        loader.modrinth_slug()
    }

    fn sort_key(sort: ModSort) -> &'static str {
        match sort {
            ModSort::Relevance => "relevance",
            ModSort::Downloads => "downloads",
            ModSort::Updated => "updated",
        }
    }

    /// Shared body of `versions()`/`plugin_versions()`: `GET
    /// /v2/project/{id}/version` with optional `loaders`/`game_versions`
    /// facet arrays, mapped to `ModVersion`. `loaders` is a list of raw
    /// Modrinth loader slugs (Java loader slug for `versions()`, plugin-core
    /// slugs for `plugin_versions()`) — the caller owns what those strings
    /// mean; this helper only builds the query and parses the response.
    async fn fetch_versions(
        &self,
        project_id: &str,
        mc: Option<&str>,
        loaders: Option<&[&str]>,
    ) -> Result<Vec<ModVersion>, Error> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(slugs) = loaders {
            // A slice of &str loader slugs always serializes to a JSON array.
            let loaders_json =
                serde_json::to_string(slugs).expect("a slice of &str always serializes");
            params.push(("loaders", urlencode(&loaders_json)));
        }
        if let Some(v) = mc {
            // A fixed one-element array of &str always serializes to a JSON array.
            let games = serde_json::to_string(&[v])
                .expect("a fixed one-element &str array always serializes");
            params.push(("game_versions", urlencode(&games)));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!(
                "?{}",
                params
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("&")
            )
        };
        let url = format!("{}/v2/project/{}/version{}", self.base, project_id, query);
        let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
        if !(200..300).contains(&resp.status) {
            return Err(Error::ModsNetwork {
                url,
                details: format!("HTTP {}", resp.status),
            });
        }
        let raws: Vec<types::Version> =
            serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                platform: "modrinth".into(),
                details: e.to_string(),
            })?;
        Ok(raws.into_iter().map(convert_version).collect())
    }

    /// Fetch each project's `server_side` support in bulk — mirrors `summaries`
    /// (`GET /v2/projects?ids=[...]`) but extracts only `id` + `server_side`.
    /// Unknown / unrecognized ids are simply absent from the returned map.
    /// Powers the new-server client-mod quarantine (Phase 1).
    pub async fn server_side_bulk(
        &self,
        ids: &[&str],
    ) -> Result<HashMap<String, ServerSideSupport>, Error> {
        #[derive(serde::Deserialize)]
        struct Sides {
            id: String,
            server_side: Option<String>,
        }
        let mut out = HashMap::new();
        for chunk in ids.chunks(BATCH_CHUNK) {
            if chunk.is_empty() {
                continue;
            }
            let ids_json = serde_json::to_string(chunk)
                .expect("a slice of ids always serializes to a JSON array");
            let url = format!("{}/v2/projects?ids={}", self.base, urlencode(&ids_json));
            let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
                .await
                .map_err(|e| Error::mods_network(url.clone(), e))?;
            if !(200..300).contains(&resp.status) {
                return Err(Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            let projects: Vec<Sides> =
                serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                    platform: "modrinth".into(),
                    details: e.to_string(),
                })?;
            for p in projects {
                out.insert(
                    p.id,
                    ServerSideSupport::from_modrinth(p.server_side.as_deref()),
                );
            }
        }
        Ok(out)
    }

    /// Resolve SHA-1 hashes to Modrinth project ids in bulk via
    /// `POST /v2/version_files`. Returns `sha1 (lowercased) -> project_id` for
    /// every hash Modrinth knows; unknown hashes are absent. No API key needed.
    /// Powers the existing-server client-mod quarantine (Phase 1).
    pub async fn project_ids_by_hash(
        &self,
        shas: &[&str],
    ) -> Result<HashMap<String, String>, Error> {
        #[derive(serde::Deserialize)]
        struct VersionLite {
            project_id: String,
        }
        let mut out = HashMap::new();
        for chunk in shas.chunks(BATCH_CHUNK) {
            if chunk.is_empty() {
                continue;
            }
            let url = format!("{}/v2/version_files", self.base);
            let body = serde_json::to_vec(&serde_json::json!({
                "hashes": chunk,
                "algorithm": "sha1",
            }))
            .expect("a fixed-shape JSON object always serializes");
            let resp = crate::network::request::post(
                &url,
                &[("user-agent", UA), ("content-type", "application/json")],
                &body,
                "mods",
            )
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
            if !(200..300).contains(&resp.status) {
                return Err(Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            let map: HashMap<String, VersionLite> =
                serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                    platform: "modrinth".into(),
                    details: e.to_string(),
                })?;
            for (sha, v) in map {
                out.insert(sha.to_ascii_lowercase(), v.project_id);
            }
        }
        Ok(out)
    }

    /// Like `project_ids_by_hash`, but returns the full matched-version
    /// identity. Same endpoint (`POST /v2/version_files`), same `BATCH_CHUNK`
    /// batching, no API key. Keyed by lowercased sha1.
    pub async fn versions_by_hashes(
        &self,
        shas: &[&str],
    ) -> Result<HashMap<String, HashVersion>, Error> {
        #[derive(serde::Deserialize)]
        struct VersionLite {
            id: String,
            project_id: String,
            version_number: String,
            name: String,
        }
        let mut out = HashMap::new();
        for chunk in shas.chunks(BATCH_CHUNK) {
            if chunk.is_empty() {
                continue;
            }
            let url = format!("{}/v2/version_files", self.base);
            let body = serde_json::to_vec(&serde_json::json!({
                "hashes": chunk,
                "algorithm": "sha1",
            }))
            .expect("a fixed-shape JSON object always serializes");
            let resp = crate::network::request::post(
                &url,
                &[("user-agent", UA), ("content-type", "application/json")],
                &body,
                "mods",
            )
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
            if !(200..300).contains(&resp.status) {
                return Err(Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            let map: HashMap<String, VersionLite> =
                serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                    platform: "modrinth".into(),
                    details: e.to_string(),
                })?;
            for (sha, v) in map {
                out.insert(
                    sha.to_ascii_lowercase(),
                    HashVersion {
                        project_id: v.project_id,
                        version_id: v.id,
                        version_number: v.version_number,
                        name: v.name,
                    },
                );
            }
        }
        Ok(out)
    }
}

impl Default for ModrinthClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ModPlatform for ModrinthClient {
    async fn search(&self, q: &ModSearchQuery) -> Result<ModSearchPage, Error> {
        let facets = build_facets(q.kind, q.mc_version.as_deref(), q.loader, q.plugin_core);
        // Serializing Vec<Vec<String>> cannot fail. Per CLAUDE.md `.unwrap()` rule.
        let facets_json = serde_json::to_string(&facets).unwrap();
        let url = format!(
            "{}/v2/search?query={}&limit={}&offset={}&index={}&facets={}",
            self.base,
            urlencode(&q.query),
            q.page_size,
            q.offset,
            Self::sort_key(q.sort),
            urlencode(&facets_json),
        );
        let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
        if resp.status == 404 {
            return Err(Error::ModsNotFound {
                platform: "modrinth".into(),
            });
        }
        if !(200..300).contains(&resp.status) {
            return Err(Error::ModsNetwork {
                url,
                details: format!("HTTP {}", resp.status),
            });
        }
        let body: types::SearchResponse =
            serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                platform: "modrinth".into(),
                details: e.to_string(),
            })?;
        Ok(ModSearchPage {
            hits: body
                .hits
                .into_iter()
                .map(|h| ModSummary {
                    source: ModSource::Modrinth,
                    project_id: h.project_id,
                    slug: Some(h.slug),
                    name: h.title,
                    summary: h.description,
                    icon_url: h.icon_url,
                    downloads: h.downloads as f64,
                    author: h.author,
                    updated_at: h.date_modified,
                    // Search interleaves loader tags into `categories`, which
                    // `SearchHit` does not parse. Search hits never enter the
                    // dependency graph (it batches `summaries()` by project id),
                    // so leaving this unknown costs nothing.
                    loaders: None,
                })
                .collect(),
            total: body.total_hits,
            offset: body.offset,
            page_size: body.limit,
        })
    }

    async fn project(&self, project_id: &str) -> Result<ModProject, Error> {
        let url = format!("{}/v2/project/{}", self.base, project_id);
        let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
        if resp.status == 404 {
            return Err(Error::ModsNotFound {
                platform: "modrinth".into(),
            });
        }
        if !(200..300).contains(&resp.status) {
            return Err(Error::ModsNetwork {
                url,
                details: format!("HTTP {}", resp.status),
            });
        }
        let p: types::Project =
            serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                platform: "modrinth".into(),
                details: e.to_string(),
            })?;
        let summary = summary_from_project(&p);
        let body_html = crate::mods::render::markdown_to_safe_html(&p.body);
        let website_url = p.source_url.or(p.wiki_url);
        let mut gallery_entries = p.gallery;
        // Featured first, then by the platform's `ordering` (None last).
        gallery_entries.sort_by(|a, b| {
            b.featured.cmp(&a.featured).then(
                a.ordering
                    .unwrap_or(i64::MAX)
                    .cmp(&b.ordering.unwrap_or(i64::MAX)),
            )
        });
        let gallery = gallery_entries
            .into_iter()
            .filter(|e| crate::mods::render::is_safe_image_url(&e.url))
            .map(|e| crate::mods::platform::GalleryImage {
                url: e.url,
                title: e.title,
            })
            .collect();
        Ok(ModProject {
            summary,
            body_html,
            gallery,
            website_url,
        })
    }

    async fn versions(
        &self,
        project_id: &str,
        mc: Option<&str>,
        loader: Option<LoaderKind>,
    ) -> Result<Vec<ModVersion>, Error> {
        let loader_slugs = loader.map(|l| vec![Self::loader_facet(l)]);
        let versions = self
            .fetch_versions(project_id, mc, loader_slugs.as_deref())
            .await?;
        // Defend against upstream loader mis-tagging (e.g. Xaero's Minimap
        // tags its NeoForge 1.20.4 builds with the `forge` loader). The
        // server-side `loaders` facet trusts that wrong tag; the filename
        // does not.
        Ok(crate::mods::platform::drop_filename_loader_mismatches(
            versions, loader,
        ))
    }

    async fn plugin_versions(
        &self,
        project_id: &str,
        mc_version: Option<&str>,
        plugin_loaders: &[&str],
    ) -> Result<Vec<ModVersion>, Error> {
        let want = if plugin_loaders.is_empty() {
            None
        } else {
            Some(plugin_loaders)
        };
        let mut versions = self.fetch_versions(project_id, mc_version, want).await?;
        // Plugin loader tags (bukkit/spigot/paper/purpur) have no `LoaderKind`
        // mapping — clear whatever `convert_version` filtered in so callers
        // never see a misleading Java-loader tag on a plugin build. The
        // filename-loader-mismatch defense is Java-loader specific (forge vs
        // neoforge tokens) and does not apply here, so it is intentionally
        // skipped.
        for v in &mut versions {
            v.loaders.clear();
        }
        Ok(versions)
    }

    async fn resolve_deps(
        &self,
        version: &ModVersion,
        mc: &str,
        loader: LoaderKind,
    ) -> Result<ResolvedDeps, Error> {
        let mut required = Vec::new();
        let mut optional = Vec::new();
        let mut incompatible = Vec::new();
        let mut unresolvable = Vec::new();
        for dep in &version.deps {
            // Incompatible / embedded deps don't need a version lookup.
            match dep.kind {
                DepKind::Incompatible => {
                    incompatible.push(dep.project_ref.clone());
                    continue;
                }
                DepKind::Embedded => continue,
                _ => {}
            }
            let (pid, pin) = match &dep.project_ref {
                DepProjectRef::Modrinth {
                    project_id,
                    version_id,
                } => (project_id.clone(), version_id.as_deref()),
                DepProjectRef::Curseforge { .. } => {
                    // Cross-source dep we can't resolve on this platform — only
                    // worth flagging when it's required.
                    if dep.kind == DepKind::Required {
                        unresolvable.push(dep.project_ref.clone());
                    }
                    continue;
                }
            };
            let versions = self.versions(&pid, Some(mc), Some(loader)).await?;
            // Honor the author-pinned `version_id` when present and compatible;
            // otherwise newest-compatible (marked). Modrinth dependency metadata
            // carries no range — pin only.
            if let Some((i, reason)) =
                crate::mods::dep_select::select_dep_version(&versions, pin, None)
            {
                let version = versions
                    .into_iter()
                    .nth(i)
                    .expect("index returned by select_dep_version is in range");
                let resolved = ResolvedDep {
                    project_ref: dep.project_ref.clone(),
                    version,
                    selection_reason: reason,
                };
                match dep.kind {
                    DepKind::Required => required.push(resolved),
                    DepKind::Optional => optional.push(resolved),
                    _ => {}
                }
            } else if dep.kind == DepKind::Required {
                // A *required* dep with no compatible build is a real problem
                // worth surfacing ("install anyway?"). An *optional* one simply
                // isn't available for this MC/loader — skip it silently rather
                // than alarm the user.
                unresolvable.push(dep.project_ref.clone());
            }
        }
        Ok(ResolvedDeps {
            required,
            optional,
            incompatible,
            unresolvable,
        })
    }

    async fn summaries(&self, ids: &[&str]) -> Result<Vec<ModSummary>, Error> {
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(BATCH_CHUNK) {
            // Modrinth: GET /v2/projects?ids=["a","b",…] → array of the same
            // Project shape as /v2/project/{id}. Unknown ids are omitted.
            // Serialising a slice of string ids to a JSON array is infallible.
            let ids_json = serde_json::to_string(chunk)
                .expect("a slice of ids always serializes to a JSON array");
            let url = format!("{}/v2/projects?ids={}", self.base, urlencode(&ids_json));
            let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
                .await
                .map_err(|e| Error::mods_network(url.clone(), e))?;
            if !(200..300).contains(&resp.status) {
                return Err(Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            let projects: Vec<types::Project> =
                serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                    platform: "modrinth".into(),
                    details: e.to_string(),
                })?;
            out.extend(projects.iter().map(summary_from_project));
        }
        Ok(out)
    }

    async fn versions_by_ids(&self, version_ids: &[&str]) -> Result<Vec<ModVersion>, Error> {
        let mut out = Vec::with_capacity(version_ids.len());
        for chunk in version_ids.chunks(BATCH_CHUNK) {
            // Modrinth: GET /v2/versions?ids=["v1","v2",…] → array of Version
            // objects, each carrying its `dependencies`. Unknown ids omitted.
            // Serialising a slice of string ids to a JSON array is infallible.
            let ids_json = serde_json::to_string(chunk)
                .expect("a slice of ids always serializes to a JSON array");
            let url = format!("{}/v2/versions?ids={}", self.base, urlencode(&ids_json));
            let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
                .await
                .map_err(|e| Error::mods_network(url.clone(), e))?;
            if !(200..300).contains(&resp.status) {
                return Err(Error::ModsNetwork {
                    url,
                    details: format!("HTTP {}", resp.status),
                });
            }
            let raws: Vec<types::Version> =
                serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                    platform: "modrinth".into(),
                    details: e.to_string(),
                })?;
            out.extend(raws.into_iter().map(convert_version));
        }
        Ok(out)
    }

    async fn changelog_range(
        &self,
        project_id: &str,
        target_version_id: &str,
        base_version_id: Option<&str>,
    ) -> Result<crate::mods::changelog::ChangelogResult, Error> {
        use crate::mods::changelog::{changelog_window, ChangelogResult, ChangelogSection};

        // The version object already carries `changelog` (markdown), so one
        // list fetch covers the whole cumulative window — no per-version calls.
        let url = format!("{}/v2/project/{}/version", self.base, project_id);
        let resp = crate::network::request::get(&url, &[("user-agent", UA)], "mods")
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
        if resp.status == 404 {
            return Err(Error::ModsNotFound {
                platform: "modrinth".into(),
            });
        }
        if !(200..300).contains(&resp.status) {
            return Err(Error::ModsNetwork {
                url,
                details: format!("HTTP {}", resp.status),
            });
        }
        let mut list: Vec<types::Version> =
            serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
                platform: "modrinth".into(),
                details: e.to_string(),
            })?;
        // Ensure newest-first so the window math is correct regardless of
        // upstream ordering (Modrinth returns date-descending, but be explicit).
        list.sort_by(|a, b| b.date_published.cmp(&a.date_published));

        // Restrict to the TARGET's release lineage — versions sharing at least
        // one loader AND one game version with the target. Without this, a
        // project that publishes many loaders / MC versions (e.g. a 1.20.4
        // backport released after newer 1.21 builds) would pull every unrelated
        // build published between `base` and `target` by date into the window.
        // The target defines the lineage, so it is always present — no empty
        // window. When the target can't be found (shouldn't happen), keep all.
        let (t_mcs, t_loaders): (Vec<String>, Vec<String>) = list
            .iter()
            .find(|v| v.id == target_version_id)
            .map(|t| (t.game_versions.clone(), t.loaders.clone()))
            .unwrap_or_default();
        let lineage: Vec<&types::Version> = if t_mcs.is_empty() && t_loaders.is_empty() {
            list.iter().collect()
        } else {
            list.iter()
                .filter(|v| {
                    v.loaders.iter().any(|l| t_loaders.contains(l))
                        && v.game_versions.iter().any(|g| t_mcs.contains(g))
                })
                .collect()
        };

        let ids: Vec<&str> = lineage.iter().map(|v| v.id.as_str()).collect();
        let (start, end, full) = changelog_window(&ids, target_version_id, base_version_id);
        let sections: Vec<ChangelogSection> = lineage[start..end]
            .iter()
            .map(|v| ChangelogSection {
                version_id: v.id.clone(),
                version_number: v.version_number.clone(),
                published_at: v.date_published.clone(),
                body_html: v
                    .changelog
                    .as_deref()
                    .map(crate::mods::render::markdown_to_safe_html)
                    .unwrap_or_default(),
            })
            .collect();
        let truncated = (end - start < full).then_some(full as u32);
        Ok(ChangelogResult {
            sections,
            truncated,
        })
    }
}

/// Map Modrinth loader slugs to `LoaderKind`, dropping every tag that is not a
/// loader we model.
///
/// Modrinth's loader vocabulary is much wider than this list — it also carries
/// `datapack`, `iris`, `optifine`, `canvas`, `rift`, `bukkit`, … Dropping them
/// is deliberate and load-bearing for the dependency graph: a shader or datapack
/// project maps to an EMPTY vec, and an empty loader set never suppresses a
/// dependency row (`local::loaders_disjoint_from_instance`). `minecraft` maps to
/// `Vanilla`, which is loader-agnostic and likewise never suppresses.
fn loaders_from_slugs(slugs: &[String]) -> Vec<LoaderKind> {
    slugs
        .iter()
        .filter_map(|s| match s.as_str() {
            "fabric" => Some(LoaderKind::Fabric),
            "quilt" => Some(LoaderKind::Quilt),
            "forge" => Some(LoaderKind::Forge),
            "neoforge" => Some(LoaderKind::NeoForge),
            "minecraft" => Some(LoaderKind::Vanilla),
            _ => None,
        })
        .collect()
}

/// Map a Modrinth `Project` to the normalized summary. Shared by `project()`
/// and the batched `summaries()` so both paths agree on the field mapping
/// (author = team, no `updated_at` — the project endpoint omits it).
fn summary_from_project(p: &types::Project) -> ModSummary {
    ModSummary {
        source: ModSource::Modrinth,
        project_id: p.id.clone(),
        slug: Some(p.slug.clone()),
        name: p.title.clone(),
        summary: p.description.clone(),
        icon_url: p.icon_url.clone(),
        downloads: p.downloads as f64,
        author: p.team.clone(),
        updated_at: None,
        // Always `Some`, empty vec included — Modrinth CAN report project
        // loaders, so `None` here would mean "unknown source" and would mark the
        // cached entry permanently stale. See `platform::ModSummary::loaders`.
        loaders: Some(loaders_from_slugs(&p.loaders)),
    }
}

/// Minimal percent-encoder for query-string values. Encodes anything outside
/// the URL-safe unreserved set [A-Za-z0-9-_.~] using uppercase %HH form.
/// We use this instead of `RequestBuilder::query()` to avoid pulling
/// `serde_urlencoded` features that are disabled by our `reqwest` flags.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        let safe = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if safe {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", b));
        }
    }
    out
}

fn convert_version(v: types::Version) -> ModVersion {
    let primary = v
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| v.files.first())
        .cloned();
    let pf = primary
        .map(|f| ModFile {
            filename: f.filename,
            url: f.url,
            sha1: Some(f.hashes.sha1),
            size: f.size as f64,
            distribution_allowed: true,
            sha256: None,
        })
        .unwrap_or(ModFile {
            filename: "missing".into(),
            url: "about:blank".into(),
            sha1: None,
            size: 0.0,
            distribution_allowed: false,
            sha256: None,
        });
    ModVersion {
        source: ModSource::Modrinth,
        project_id: v.project_id,
        version_id: v.id,
        name: v.name,
        version_number: v.version_number,
        mc_versions: v.game_versions,
        loaders: loaders_from_slugs(&v.loaders),
        primary_file: pf,
        deps: v
            .dependencies
            .into_iter()
            .filter_map(|d| {
                let kind = match d.dependency_type.as_str() {
                    "required" => DepKind::Required,
                    "optional" => DepKind::Optional,
                    "incompatible" => DepKind::Incompatible,
                    "embedded" => DepKind::Embedded,
                    _ => return None,
                };
                let project_ref = DepProjectRef::Modrinth {
                    project_id: d.project_id?,
                    version_id: d.version_id,
                };
                Some(ModDepLink { kind, project_ref })
            })
            .collect(),
        published_at: v.date_published,
    }
}

fn project_type_facet(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Mod => "project_type:mod",
        ContentKind::ResourcePack => "project_type:resourcepack",
        ContentKind::Shader => "project_type:shader",
        ContentKind::Plugin => "project_type:plugin",
        // Undocumented but real: Modrinth's documented facet values are
        // mod/modpack/resourcepack/shader, yet `project_type:datapack`
        // genuinely filters — measured 2026-08-04 against /v2/search, 13 804
        // hits versus 148 896 unfaceted and 0 for a nonsense value, so unknown
        // values filter to nothing rather than being ignored.
        //
        // Note the hits report `project_type: "mod"` in their own payload:
        // Modrinth's project_type is version-specific while the facet spans
        // every type a project publishes. A hybrid such as Terralith ships
        // both a datapack and a mod, which is why VERSION listing must filter
        // by the `datapack` loader slug — see `datapack_versions`.
        ContentKind::Datapack => "project_type:datapack",
    }
}

fn build_facets(
    kind: ContentKind,
    mc_version: Option<&str>,
    loader: Option<LoaderKind>,
    plugin_core: Option<crate::servers_runtime::schema::ServerCore>,
) -> Vec<Vec<String>> {
    let mut facets: Vec<Vec<String>> = vec![vec![project_type_facet(kind).into()]];
    if let Some(mc) = mc_version {
        facets.push(vec![format!("versions:{mc}")]);
    }
    match kind {
        // The Java loader facet applies to mods ONLY. Resource packs have no
        // loader, and Modrinth shader categories are iris/optifine/canvas —
        // passing `categories:<loader>` to a shader search returns almost
        // nothing.
        ContentKind::Mod => {
            if let Some(l) = loader {
                facets.push(vec![format!(
                    "categories:{}",
                    ModrinthClient::loader_facet(l)
                )]);
            }
        }
        // Plugin loader compatibility is an OR-group: a plugin tagged with
        // ANY of the core's compatible slugs (e.g. Purpur accepts
        // bukkit/spigot/paper/purpur-tagged plugins) is a match. Skipped
        // entirely when the caller passed no core, or the core is somehow
        // not plugin-capable (empty slug list).
        ContentKind::Plugin => {
            if let Some(core) = plugin_core {
                let slugs = core.plugin_loader_slugs();
                if !slugs.is_empty() {
                    facets.push(
                        slugs
                            .iter()
                            .map(|s| format!("categories:{s}"))
                            .collect::<Vec<_>>(),
                    );
                }
            }
        }
        // Datapacks join resource packs and shaders here: they have no Java
        // loader, so no `categories:` facet applies. Modrinth does tag them
        // with a `datapack` loader, but that belongs on the VERSION query
        // (`datapack_versions`), not on search — the search facet is already
        // `project_type:datapack`, and adding a second constraint here would
        // only narrow it redundantly.
        ContentKind::ResourcePack | ContentKind::Shader | ContentKind::Datapack => {}
    }
    facets
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    #[test]
    fn facets_use_project_type_per_kind_and_skip_loader_for_resourcepack() {
        let f = build_facets(
            ContentKind::Mod,
            Some("1.20.4"),
            Some(LoaderKind::Fabric),
            None,
        );
        assert!(f.contains(&vec!["project_type:mod".to_string()]));
        assert!(f
            .iter()
            .any(|g| g.iter().any(|s| s.starts_with("categories:"))));
        assert!(f.contains(&vec!["versions:1.20.4".to_string()]));

        let f = build_facets(
            ContentKind::ResourcePack,
            Some("1.20.4"),
            Some(LoaderKind::Fabric),
            None,
        );
        assert!(f.contains(&vec!["project_type:resourcepack".to_string()]));
        assert!(!f
            .iter()
            .any(|g| g.iter().any(|s| s.starts_with("categories:"))));

        // Shaders use iris/optifine/canvas categories, NOT the Java loader.
        // Passing `categories:fabric` to a shader search returns almost nothing,
        // so the loader facet must be omitted for shaders (mods only).
        let f = build_facets(ContentKind::Shader, None, Some(LoaderKind::Fabric), None);
        assert!(f.contains(&vec!["project_type:shader".to_string()]));
        assert!(!f
            .iter()
            .any(|g| g.iter().any(|s| s.starts_with("categories:"))));
    }

    #[test]
    fn datapack_facets_use_the_datapack_project_type_and_no_categories() {
        // `project_type:datapack` is absent from Modrinth's documented facet
        // values (mod/modpack/resourcepack/shader) but genuinely filters —
        // measured against /v2/search on 2026-08-04: 13 804 hits, versus
        // 148 896 with no facet and 0 for `project_type:nonsense_xyz`. An
        // unknown value filters to nothing rather than being ignored, which is
        // what proves the facet is live rather than silently dropped.
        let f = build_facets(ContentKind::Datapack, Some("1.21.4"), Some(LoaderKind::Fabric), None);
        assert!(f.contains(&vec!["project_type:datapack".to_string()]));
        assert!(f.contains(&vec!["versions:1.21.4".to_string()]));
        // A datapack has no Java loader. The `datapack` loader tag belongs on
        // the version query, not on search.
        assert!(
            !f.iter()
                .any(|g| g.iter().any(|s| s.starts_with("categories:"))),
            "datapack search must carry no categories facet: {f:?}"
        );
    }

    #[test]
    fn plugin_facets_use_plugin_project_type_and_core_or_group() {
        use crate::servers_runtime::schema::ServerCore;
        let f = build_facets(
            ContentKind::Plugin,
            Some("1.21.4"),
            None,
            Some(ServerCore::Paper),
        );
        assert_eq!(
            f,
            vec![
                vec!["project_type:plugin".to_string()],
                vec!["versions:1.21.4".to_string()],
                vec![
                    "categories:bukkit".to_string(),
                    "categories:spigot".to_string(),
                    "categories:paper".to_string(),
                ],
            ]
        );
        let f = build_facets(
            ContentKind::Plugin,
            Some("1.21.4"),
            None,
            Some(ServerCore::Purpur),
        );
        assert_eq!(f[2].len(), 4);
        assert!(f[2].contains(&"categories:purpur".to_string()));
    }

    #[test]
    fn mod_facets_unchanged_by_plugin_core_param() {
        let f = build_facets(
            ContentKind::Mod,
            Some("1.20.1"),
            Some(LoaderKind::Fabric),
            None,
        );
        assert_eq!(
            f,
            vec![
                vec!["project_type:mod".to_string()],
                vec!["versions:1.20.1".to_string()],
                vec!["categories:fabric".to_string()],
            ]
        );
    }

    async fn server() -> MockServer {
        MockServer::start().await
    }

    #[tokio::test]
    async fn summaries_batches_projects_in_one_request() {
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/projects"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[
                  {"id":"jei","slug":"jei","title":"JEI","description":"Items",
                   "body":"","icon_url":"https://media.modrinth.com/i.png","downloads":10,
                   "source_url":null,"wiki_url":null,"team":"t","gallery":[],
                   "loaders":["fabric","neoforge","iris"]},
                  {"id":"sodium","slug":"sodium","title":"Sodium","description":"Perf",
                   "body":"","icon_url":null,"downloads":99,
                   "source_url":null,"wiki_url":null,"team":"jelly","gallery":[]}
                ]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let out = c.summaries(&["jei", "sodium"]).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "JEI");
        assert_eq!(out[0].slug.as_deref(), Some("jei"));
        assert_eq!(out[1].author, "jelly");
        // The batch endpoint carries project-level `loaders`; unknown tags
        // ("iris") are dropped rather than failing the decode.
        assert_eq!(
            out[0].loaders,
            Some(vec![LoaderKind::Fabric, LoaderKind::NeoForge])
        );
        // Absent key ⇒ empty vec, NOT None: Modrinth *can* report loaders, and
        // `None` would mark the cached entry permanently stale.
        assert_eq!(
            out[1].loaders,
            Some(Vec::new()),
            "a project with no loaders key must be Some(empty), never None"
        );
    }

    #[tokio::test]
    async fn summaries_empty_ids_makes_no_request() {
        let _g = test_lock();
        // No mocks mounted — any outbound request would 404 and the call would
        // error. An empty input must short-circuit to Ok(empty).
        let s = server().await;
        let c = ModrinthClient::with_base(s.uri());
        let out = c.summaries(&[]).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn versions_by_ids_parses_versions_with_deps() {
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{
                  "id":"vid1","project_id":"jei","name":"JEI 15","version_number":"15.0.0",
                  "game_versions":["1.20.1"],"loaders":["fabric"],"date_published":"2026-05-01T00:00:00Z",
                  "files":[{"url":"https://cdn/x.jar","filename":"jei.jar","hashes":{"sha1":"abc"},"size":1,"primary":true}],
                  "dependencies":[{"project_id":"fabric-api","version_id":null,"dependency_type":"required"}]
                }]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let out = c.versions_by_ids(&["vid1"]).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].project_id, "jei");
        assert_eq!(out[0].deps.len(), 1);
        assert_eq!(out[0].deps[0].kind, DepKind::Required);
    }

    #[tokio::test]
    async fn search_parses_hits() {
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                "hits": [{
                    "project_id":"u6dRKJwZ","slug":"jei","title":"JEI","description":"Items",
                    "icon_url":null,"downloads":1234,"author":"mezz","date_modified":"2026-05-01T00:00:00Z"
                }],
                "total_hits":1,"offset":0,"limit":20
            }"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let q = ModSearchQuery {
            source: ModSource::Modrinth,
            kind: ContentKind::Mod,
            query: "jei".into(),
            mc_version: Some("1.20.1".into()),
            loader: Some(LoaderKind::Fabric),
            sort: ModSort::Downloads,
            page_size: 20,
            offset: 0,
            plugin_core: None,
        };
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = c.search(&q).await.unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.hits[0].name, "JEI");
        assert_eq!(page.hits[0].project_id, "u6dRKJwZ");
        assert_eq!(page.hits[0].source, ModSource::Modrinth);
    }

    #[tokio::test]
    async fn search_5xx_maps_to_network_error() {
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/search"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let q = ModSearchQuery {
            source: ModSource::Modrinth,
            kind: ContentKind::Mod,
            query: "x".into(),
            mc_version: None,
            loader: None,
            sort: ModSort::Relevance,
            page_size: 20,
            offset: 0,
            plugin_core: None,
        };
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let err = c.search(&q).await.unwrap_err();
        assert!(matches!(err, Error::ModsNetwork { .. }), "got: {err:?}");
    }

    #[tokio::test]
    async fn project_404_maps_to_not_found() {
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let err = c.project("missing").await.unwrap_err();
        assert!(matches!(err, Error::ModsNotFound { .. }), "got: {err:?}");
    }

    #[tokio::test]
    async fn project_renders_body_and_orders_gallery() {
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/jei"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r##"{"id":"u6dRKJwZ","slug":"jei","title":"JEI","description":"Items",
                   "body":"# Hello\n\n![s](https://media.modrinth.com/b.png)",
                   "icon_url":null,"downloads":10,"source_url":null,"wiki_url":null,"team":"t",
                   "gallery":[
                     {"url":"https://media.modrinth.com/a.png","title":"A","featured":false,"ordering":2},
                     {"url":"https://media.modrinth.com/f.png","title":"F","featured":true,"ordering":9}
                   ]}"##,
            ))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let c = ModrinthClient::with_base(s.uri());
        let p = c.project("jei").await.unwrap();
        assert!(p.body_html.contains("<h1>"));
        assert!(p.body_html.contains("https://media.modrinth.com/b.png"));
        // Featured image sorts first regardless of ordering value.
        assert_eq!(p.gallery[0].url, "https://media.modrinth.com/f.png");
        assert_eq!(p.gallery[1].url, "https://media.modrinth.com/a.png");
    }

    #[tokio::test]
    async fn versions_parses_primary_file_and_deps() {
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/jei/version"))
            .and(query_param("loaders", r#"["fabric"]"#))
            .and(query_param("game_versions", r#"["1.20.1"]"#))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{
                "id":"vid1","project_id":"jei","name":"JEI 15","version_number":"15.0.0",
                "game_versions":["1.20.1"],"loaders":["fabric"],"date_published":"2026-05-01T00:00:00Z",
                "files":[{"url":"https://cdn.modrinth.com/x.jar","filename":"jei-15.0.0.jar",
                          "hashes":{"sha1":"abc"},"size":100,"primary":true}],
                "dependencies":[{"project_id":"fabric-api","version_id":null,"dependency_type":"required"}]
            }]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let vs = c
            .versions("jei", Some("1.20.1"), Some(LoaderKind::Fabric))
            .await
            .unwrap();
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].primary_file.filename, "jei-15.0.0.jar");
        assert_eq!(vs[0].primary_file.sha1.as_deref(), Some("abc"));
        assert_eq!(vs[0].deps.len(), 1);
        assert_eq!(vs[0].deps[0].kind, DepKind::Required);
    }

    #[tokio::test]
    async fn changelog_range_windows_and_renders_markdown() {
        let s = server().await;
        // newest → oldest, each with a markdown changelog.
        Mock::given(method("GET"))
            .and(path("/v2/project/sodium/version"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r##"[
                  {"id":"v3","project_id":"sodium","name":"0.6.0","version_number":"0.6.0",
                   "game_versions":["1.21.4"],"loaders":["fabric"],"date_published":"2026-06-03T00:00:00Z",
                   "files":[],"dependencies":[],"changelog":"# 0.6.0\n- new renderer"},
                  {"id":"v2","project_id":"sodium","name":"0.5.9","version_number":"0.5.9",
                   "game_versions":["1.21.4"],"loaders":["fabric"],"date_published":"2026-05-01T00:00:00Z",
                   "files":[],"dependencies":[],"changelog":"- bugfixes"},
                  {"id":"v1","project_id":"sodium","name":"0.5.8","version_number":"0.5.8",
                   "game_versions":["1.21.4"],"loaders":["fabric"],"date_published":"2026-04-01T00:00:00Z",
                   "files":[],"dependencies":[],"changelog":null}
                ]"##,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // installed v1, target v3 → sections for v3 and v2 (v1 excluded).
        let res = c.changelog_range("sodium", "v3", Some("v1")).await.unwrap();
        assert_eq!(res.sections.len(), 2);
        assert_eq!(res.sections[0].version_id, "v3");
        assert!(
            res.sections[0].body_html.contains("<h1>"),
            "markdown rendered to HTML"
        );
        assert_eq!(res.sections[1].version_id, "v2");
        assert_eq!(res.truncated, None);
    }

    #[tokio::test]
    async fn changelog_range_base_none_returns_only_target() {
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/sodium/version"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[
                  {"id":"v3","project_id":"sodium","name":"0.6.0","version_number":"0.6.0",
                   "game_versions":["1.21.4"],"loaders":["fabric"],"date_published":"2026-06-03T00:00:00Z",
                   "files":[],"dependencies":[],"changelog":"notes"},
                  {"id":"v2","project_id":"sodium","name":"0.5.9","version_number":"0.5.9",
                   "game_versions":["1.21.4"],"loaders":["fabric"],"date_published":"2026-05-01T00:00:00Z",
                   "files":[],"dependencies":[],"changelog":"old"}
                ]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let res = c.changelog_range("sodium", "v3", None).await.unwrap();
        assert_eq!(res.sections.len(), 1);
        assert_eq!(res.sections[0].version_id, "v3");
    }

    #[tokio::test]
    async fn changelog_range_restricts_to_target_loader_and_mc_lineage() {
        // Real ImmediatelyFast shape: a 1.20.4-neoforge backport published AFTER
        // newer fabric/26.2 builds. Windowing by date alone would drag the fabric
        // build into the range; the lineage filter (target's loader + MC) must
        // keep only the 1.20.4-neoforge releases.
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/imf/version"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r##"[
                  {"id":"n5","project_id":"imf","name":"1.5.5","version_number":"1.5.5+1.20.4-neoforge",
                   "game_versions":["1.20.4"],"loaders":["neoforge"],"date_published":"2026-06-30T00:00:00Z",
                   "files":[],"dependencies":[],"changelog":"neoforge 1.5.5"},
                  {"id":"fab","project_id":"imf","name":"1.16.1","version_number":"1.16.1+26.2-fabric",
                   "game_versions":["26.2"],"loaders":["fabric"],"date_published":"2026-06-27T00:00:00Z",
                   "files":[],"dependencies":[],"changelog":"fabric build"},
                  {"id":"n4","project_id":"imf","name":"1.5.4","version_number":"1.5.4+1.20.4-neoforge",
                   "game_versions":["1.20.4"],"loaders":["neoforge"],"date_published":"2026-05-01T00:00:00Z",
                   "files":[],"dependencies":[],"changelog":"neoforge 1.5.4"},
                  {"id":"n3","project_id":"imf","name":"1.5.3","version_number":"1.5.3+1.20.4-neoforge",
                   "game_versions":["1.20.4"],"loaders":["neoforge"],"date_published":"2026-04-01T00:00:00Z",
                   "files":[],"dependencies":[],"changelog":"neoforge 1.5.3"}
                ]"##,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // installed n3, target n5 → only n5, n4 (base n3 excluded); fabric build dropped.
        let res = c.changelog_range("imf", "n5", Some("n3")).await.unwrap();
        let ids: Vec<&str> = res.sections.iter().map(|s| s.version_id.as_str()).collect();
        assert_eq!(ids, vec!["n5", "n4"]);
        assert_eq!(res.truncated, None);
    }

    #[tokio::test]
    async fn versions_drops_neoforge_jar_mistagged_as_forge() {
        // Real Xaero's Minimap 1.20.4 data: the author tags BOTH the Forge and
        // the NeoForge build with the `forge` loader, and the NeoForge build is
        // newest (so it sorts first). A Forge request must not install it.
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/xaeros-minimap/version"))
            .and(query_param("loaders", r#"["forge"]"#))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[
                {"id":"v1","project_id":"xaeros-minimap","name":"neoforge-1.20.4-25.3.13",
                 "version_number":"neoforge-1.20.4-25.3.13","game_versions":["1.20.4"],
                 "loaders":["forge"],"date_published":"2026-03-13T00:00:00Z",
                 "files":[{"url":"https://cdn/n.jar","filename":"xaerominimap-neoforge-1.20.4-25.3.13.jar",
                           "hashes":{"sha1":"n1"},"size":100,"primary":true}],
                 "dependencies":[]},
                {"id":"v2","project_id":"xaeros-minimap","name":"forge-1.20.4-25.3.13",
                 "version_number":"forge-1.20.4-25.3.13","game_versions":["1.20.4"],
                 "loaders":["forge"],"date_published":"2026-03-13T00:00:00Z",
                 "files":[{"url":"https://cdn/f.jar","filename":"xaerominimap-forge-1.20.4-25.3.13.jar",
                           "hashes":{"sha1":"f1"},"size":100,"primary":true}],
                 "dependencies":[]}
            ]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let vs = c
            .versions("xaeros-minimap", Some("1.20.4"), Some(LoaderKind::Forge))
            .await
            .unwrap();
        assert_eq!(vs.len(), 1, "the mis-tagged NeoForge jar must be dropped");
        assert_eq!(
            vs[0].primary_file.filename,
            "xaerominimap-forge-1.20.4-25.3.13.jar"
        );
    }

    #[tokio::test]
    async fn resolve_deps_flags_only_required_when_no_compatible_version() {
        let s = server().await;
        // Neither dep has a compatible build (both endpoints return []).
        for pid in ["optdep", "reqdep"] {
            Mock::given(method("GET"))
                .and(path(format!("/v2/project/{pid}/version")))
                .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
                .mount(&s)
                .await;
        }
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

        let v = ModVersion {
            source: ModSource::Modrinth,
            project_id: "primary".into(),
            version_id: "vp".into(),
            name: "Primary".into(),
            version_number: "1.0".into(),
            mc_versions: vec!["1.20.4".into()],
            loaders: vec![LoaderKind::Forge],
            primary_file: ModFile {
                filename: "p.jar".into(),
                url: "https://cdn/p.jar".into(),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: vec![
                ModDepLink {
                    kind: DepKind::Optional,
                    project_ref: DepProjectRef::Modrinth {
                        project_id: "optdep".into(),
                        version_id: None,
                    },
                },
                ModDepLink {
                    kind: DepKind::Required,
                    project_ref: DepProjectRef::Modrinth {
                        project_id: "reqdep".into(),
                        version_id: None,
                    },
                },
            ],
            published_at: None,
        };
        let rd = c
            .resolve_deps(&v, "1.20.4", LoaderKind::Forge)
            .await
            .unwrap();

        // The optional dep with no compatible build is skipped silently; only
        // the missing *required* dep is surfaced as unresolvable.
        assert!(rd.optional.is_empty(), "optional dep should not resolve");
        assert!(rd.required.is_empty(), "required dep has no build");
        assert_eq!(rd.unresolvable.len(), 1, "only the required dep is flagged");
        match &rd.unresolvable[0] {
            DepProjectRef::Modrinth { project_id, .. } => assert_eq!(project_id, "reqdep"),
            other => panic!("expected modrinth reqdep ref, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_deps_honors_author_pinned_version_id() {
        let s = server().await;
        // Dependency project "lib" has two compatible builds; the author pinned
        // the OLDER one. The version endpoint returns newest-first.
        Mock::given(method("GET"))
            .and(path("/v2/project/lib/version"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[
                  {"id":"lib-new","project_id":"lib","name":"Lib 2","version_number":"2.0.0",
                   "game_versions":["1.20.1"],"loaders":["forge"],"date_published":"2026-05-01T00:00:00Z",
                   "files":[{"url":"https://cdn/new.jar","filename":"lib-2.jar","hashes":{"sha1":"bb"},"size":1,"primary":true}],
                   "dependencies":[]},
                  {"id":"lib-old","project_id":"lib","name":"Lib 1","version_number":"1.0.0",
                   "game_versions":["1.20.1"],"loaders":["forge"],"date_published":"2026-01-01T00:00:00Z",
                   "files":[{"url":"https://cdn/old.jar","filename":"lib-1.jar","hashes":{"sha1":"cc"},"size":1,"primary":true}],
                   "dependencies":[]}
                ]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);

        let primary = ModVersion {
            source: ModSource::Modrinth,
            project_id: "primary".into(),
            version_id: "vp".into(),
            name: "Primary".into(),
            version_number: "1.0".into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![LoaderKind::Forge],
            primary_file: ModFile {
                filename: "p.jar".into(),
                url: "https://cdn/p.jar".into(),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: vec![ModDepLink {
                kind: DepKind::Required,
                project_ref: DepProjectRef::Modrinth {
                    project_id: "lib".into(),
                    version_id: Some("lib-old".into()),
                },
            }],
            published_at: None,
        };
        let rd = c
            .resolve_deps(&primary, "1.20.1", LoaderKind::Forge)
            .await
            .unwrap();
        assert_eq!(rd.required.len(), 1);
        assert_eq!(rd.required[0].version.version_id, "lib-old");
        assert_eq!(rd.required[0].selection_reason, SelectionReason::PinHonored);
    }

    #[tokio::test]
    async fn server_side_bulk_extracts_support_per_project() {
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/projects"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[
                  {"id":"betterf3","server_side":"unsupported"},
                  {"id":"jei","server_side":"optional"},
                  {"id":"voicechat","server_side":"required"},
                  {"id":"weird","server_side":"unknown"},
                  {"id":"nofield"}
                ]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let m = c
            .server_side_bulk(&["betterf3", "jei", "voicechat", "weird", "nofield"])
            .await
            .unwrap();
        assert_eq!(m.get("betterf3"), Some(&ServerSideSupport::Unsupported));
        assert_eq!(m.get("jei"), Some(&ServerSideSupport::Optional));
        assert_eq!(m.get("voicechat"), Some(&ServerSideSupport::Required));
        assert_eq!(m.get("weird"), Some(&ServerSideSupport::Unknown));
        // A project that omits `server_side` decodes as Unknown, not an error.
        assert_eq!(m.get("nofield"), Some(&ServerSideSupport::Unknown));
    }

    #[tokio::test]
    async fn server_side_bulk_empty_ids_makes_no_request() {
        let _g = test_lock();
        // No mock mounted — an empty input must short-circuit to Ok(empty).
        let s = server().await;
        let c = ModrinthClient::with_base(s.uri());
        let m = c.server_side_bulk(&[]).await.unwrap();
        assert!(m.is_empty());
    }

    #[tokio::test]
    async fn project_ids_by_hash_maps_sha_to_project_lowercased() {
        let s = server().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                  "aabbcc":{"project_id":"betterf3","id":"v1"},
                  "ddeeff":{"project_id":"jei","id":"v2"}
                }"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let m = c.project_ids_by_hash(&["AABBCC", "ddeeff"]).await.unwrap();
        assert_eq!(m.get("aabbcc"), Some(&"betterf3".to_string()));
        assert_eq!(m.get("ddeeff"), Some(&"jei".to_string()));
    }

    #[tokio::test]
    async fn versions_by_hashes_returns_full_identity() {
        let s = server().await;
        Mock::given(method("POST"))
            .and(path("/v2/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                  "aabbcc":{"project_id":"betterf3","id":"v1","version_number":"1.0.0","name":"v1"}
                }"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // Uppercase input: the returned map key is lowercased.
        let out = c.versions_by_hashes(&["AABBCC"]).await.unwrap();
        let hit = out.get("aabbcc").unwrap();
        assert_eq!(hit.project_id, "betterf3");
        assert_eq!(hit.version_id, "v1");
        assert_eq!(hit.version_number, "1.0.0");
        assert_eq!(hit.name, "v1");
    }

    #[tokio::test]
    async fn plugin_versions_queries_core_slugs() {
        let s = server().await;
        Mock::given(method("GET"))
            .and(path("/v2/project/luckperms/version"))
            .and(query_param(
                "loaders",
                r#"["bukkit","spigot","paper"]"#,
            ))
            .and(query_param("game_versions", r#"["1.21.4"]"#))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{
                    "id":"v1","project_id":"luckperms","name":"LuckPerms 5.5",
                    "version_number":"5.5.0","game_versions":["1.21.4"],
                    "loaders":["paper","bukkit"],"date_published":"2026-06-01T00:00:00Z",
                    "dependencies":[],
                    "files":[{"filename":"LuckPerms-5.5.0.jar","url":"https://cdn.modrinth.com/x.jar",
                              "primary":true,"size":1000,
                              "hashes":{"sha1":"aa","sha512":"bb"}}]
                }]"#,
            ))
            .mount(&s)
            .await;
        let c = ModrinthClient::with_base(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let vs = c
            .plugin_versions("luckperms", Some("1.21.4"), &["bukkit", "spigot", "paper"])
            .await
            .unwrap();
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].version_id, "v1");
        assert!(
            vs[0].loaders.is_empty(),
            "plugin loaders don't map to LoaderKind"
        );
        assert_eq!(vs[0].primary_file.sha1.as_deref(), Some("aa"));
    }
}

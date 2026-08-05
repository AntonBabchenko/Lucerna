//! CurseForge Eternal API client (v1).

pub mod keyring;
// `pub(crate)` so the modpack client can reuse the same `gameVersions` tag
// vocabulary instead of writing a second, drifting copy of it.
pub(crate) mod types;

use async_trait::async_trait;

use crate::error::Error;
use crate::mods::platform::*;

const BASE_DEFAULT: &str = "https://api.curseforge.com";

/// CurseForge's `/v1/mods/search` caps `pageSize` at 50 and returns HTTP 400
/// for anything larger. The shared browser UI offers up to 100 results/page
/// (Modrinth allows it), so a larger request is fanned out into back-to-back
/// windows of this size — see `search()`.
const CF_MAX_PAGE_SIZE: u32 = 50;

pub struct CurseForgeClient {
    base: String,
    api_key: Option<String>,
}

impl CurseForgeClient {
    pub fn new() -> Self {
        Self {
            base: BASE_DEFAULT.into(),
            api_key: keyring::resolve(),
        }
    }

    pub fn with_base_and_key(base: impl Into<String>, key: Option<String>) -> Self {
        Self {
            base: base.into(),
            api_key: key,
        }
    }

    /// The API key, validated as header-safe. `Missing` when no key is
    /// stored; `Invalid` when the stored key contains control characters
    /// (a paste error — a real key is an opaque printable token).
    fn auth(&self) -> Result<&str, Error> {
        let k = self.api_key.as_deref().ok_or(Error::ModsPlatformAuth {
            kind: crate::error::ModsAuthKind::Missing,
        })?;
        if k.chars().any(|c| c.is_control()) {
            return Err(Error::ModsPlatformAuth {
                kind: crate::error::ModsAuthKind::Invalid,
            });
        }
        Ok(k)
    }

    fn map_status<T: serde::de::DeserializeOwned>(
        &self,
        resp: crate::network::request::HttpResponse,
        url: String,
    ) -> Result<T, Error> {
        if resp.status == 401 || resp.status == 403 {
            keyring::clear().ok();
            return Err(Error::ModsPlatformAuth {
                kind: crate::error::ModsAuthKind::Invalid,
            });
        }
        if resp.status == 404 {
            return Err(Error::ModsNotFound {
                platform: "curseforge".into(),
            });
        }
        if !(200..300).contains(&resp.status) {
            return Err(Error::ModsNetwork {
                url,
                details: format!("HTTP {}", resp.status),
            });
        }
        serde_json::from_slice(&resp.body).map_err(|e| Error::ModsDecode {
            platform: "curseforge".into(),
            details: e.to_string(),
        })
    }

    /// Resolve `(Murmur2, sha1)` pairs to CF files via `POST /v1/fingerprints`,
    /// returned keyed by lowercased sha1. Authenticates via `self.auth()`. The
    /// response's `exactMatches[].file` carries the identity (`modId` +
    /// `id`) and echoes the `fileFingerprint` we sent, which keys each match
    /// back to its sha1.
    pub async fn files_by_fingerprint(
        &self,
        fingerprints: &[(u32, String)],
    ) -> Result<std::collections::HashMap<String, FingerprintFile>, Error> {
        let mut out = std::collections::HashMap::new();
        if fingerprints.is_empty() {
            return Ok(out);
        }
        let key = self.auth()?;
        let fp_to_sha: std::collections::HashMap<u32, String> = fingerprints
            .iter()
            .map(|(fp, sha)| (*fp, sha.clone()))
            .collect();
        let fps: Vec<u32> = fingerprints.iter().map(|(fp, _)| *fp).collect();
        let url = format!("{}/v1/fingerprints", self.base);
        let body = serde_json::to_vec(&serde_json::json!({ "fingerprints": fps }))
            .expect("a fixed-shape JSON object always serializes");
        let resp = crate::network::request::post(
            &url,
            &[("x-api-key", key), ("content-type", "application/json")],
            &body,
            "mods",
        )
        .await
        .map_err(|e| Error::mods_network(url.clone(), e))?;
        let parsed: CfFingerprintEnvelope = self.map_status(resp, url)?;
        for m in parsed.data.exact_matches {
            if let Some(sha) = fp_to_sha.get(&m.file.file_fingerprint) {
                out.insert(
                    sha.to_ascii_lowercase(),
                    FingerprintFile {
                        project_id: m.file.mod_id.to_string(),
                        version_id: m.file.id.to_string(),
                        version_number: if m.file.display_name.is_empty() {
                            None
                        } else {
                            Some(m.file.display_name)
                        },
                    },
                );
            }
        }
        Ok(out)
    }
}

/// Response-mirror structs for `POST /v1/fingerprints`, mirroring the private
/// deserialize shapes in `mods/enrich.rs` (`rename_all = "camelCase"`), plus a
/// `display_name` field (`displayName` on the CF file object).
#[derive(serde::Deserialize)]
struct CfFingerprintEnvelope {
    data: CfFingerprintData,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFingerprintData {
    exact_matches: Vec<CfFingerprintMatch>,
}

#[derive(serde::Deserialize)]
struct CfFingerprintMatch {
    file: CfFingerprintFile,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFingerprintFile {
    /// CurseForge file id == the version identity.
    id: u32,
    /// CurseForge mod id == the project identity.
    mod_id: u32,
    /// The Murmur2 fingerprint we sent, echoed back — keys this match to its jar.
    file_fingerprint: u32,
    /// CurseForge `displayName`; absent decodes to empty (dropped to `None`).
    #[serde(default)]
    display_name: String,
}

/// A CurseForge file matched by Murmur2 fingerprint.
#[derive(Debug, Clone)]
pub struct FingerprintFile {
    pub project_id: String,
    pub version_id: String,
    pub version_number: Option<String>,
}

impl Default for CurseForgeClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ModPlatform for CurseForgeClient {
    async fn search(&self, q: &ModSearchQuery) -> Result<ModSearchPage, Error> {
        // CurseForge hosts no plugin registry — the FE never offers CF as a
        // plugin source, but guard defensively rather than sending a
        // meaningless classId.
        if q.kind == ContentKind::Plugin {
            return Ok(ModSearchPage {
                hits: Vec::new(),
                total: 0,
                offset: q.offset,
                page_size: q.page_size,
            });
        }
        // Pre-validate auth so the missing-key path doesn't bother hitting
        // the network.
        let auth = self.auth()?;

        // CurseForge caps pageSize at CF_MAX_PAGE_SIZE; a UI request for more
        // (the shared browser offers up to 100, which Modrinth allows) is
        // served by stitching back-to-back windows that tile [offset, offset +
        // page_size). page_size <= the cap stays a single request, exactly as
        // before. Windows stop early once one underfills (end of results).
        //
        // CF also enforces index + pageSize <= 10_000; a deep-page request
        // (roughly page ~200 at 50/page) then returns HTTP 400, surfacing as a
        // ModsNetwork error — acceptable, as CF's own catalogue stops paging
        // there too.
        let want = q.page_size.max(1);
        let mut hits: Vec<ModSummary> = Vec::with_capacity(want as usize);
        let mut total = 0u32;
        let mut fetched = 0u32;
        while fetched < want {
            let chunk = (want - fetched).min(CF_MAX_PAGE_SIZE);
            let index = q.offset + fetched;
            let mut params: Vec<(&str, String)> = vec![
                ("gameId", types::GAME_MINECRAFT.to_string()),
                ("searchFilter", q.query.clone()),
                ("pageSize", chunk.to_string()),
                ("index", index.to_string()),
            ];
            params.push(("classId", types::class_id(q.kind).to_string()));
            if let Some(mc) = &q.mc_version {
                params.push(("gameVersion", mc.clone()));
            }
            if q.kind == crate::mods::platform::ContentKind::Mod {
                if let Some(l) = q.loader {
                    params.push(("modLoaderType", types::loader_type(l).to_string()));
                }
            }
            match q.sort {
                ModSort::Downloads => {
                    params.push(("sortField", "6".into()));
                    params.push(("sortOrder", "desc".into()));
                }
                ModSort::Updated => {
                    params.push(("sortField", "3".into()));
                    params.push(("sortOrder", "desc".into()));
                }
                ModSort::Relevance => {}
            }
            let url = format!("{}/v1/mods/search?{}", self.base, encode_pairs(&params));
            let resp = crate::network::request::get(&url, &[("x-api-key", auth)], "mods")
                .await
                .map_err(|e| Error::mods_network(url.clone(), e))?;
            let env: types::ListEnvelope<types::Mod> = self.map_status(resp, url)?;
            let got = env.data.len() as u32;
            total = env
                .pagination
                .as_ref()
                .map(|p| p.total_count)
                .unwrap_or(fetched + got);
            hits.extend(env.data.into_iter().map(convert_mod_summary));
            fetched += got;
            // A short window means the catalogue is exhausted — stop rather
            // than firing a guaranteed-empty follow-up request.
            if got < chunk {
                break;
            }
        }
        Ok(ModSearchPage {
            hits,
            total,
            offset: q.offset,
            page_size: q.page_size,
        })
    }

    async fn project(&self, project_id: &str) -> Result<ModProject, Error> {
        let auth = self.auth()?;
        let url = format!("{}/v1/mods/{}", self.base, project_id);
        let resp = crate::network::request::get(&url, &[("x-api-key", auth)], "mods")
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
        let env: types::Envelope<types::Mod> = self.map_status(resp, url)?;
        let mut m = env.data;
        let screenshots = std::mem::take(&mut m.screenshots);
        let gallery = screenshots
            .into_iter()
            .filter(|s| crate::mods::render::is_safe_image_url(&s.url))
            .map(|s| crate::mods::platform::GalleryImage {
                url: s.url,
                title: s.title,
            })
            .collect();
        // CF mod summary discards `links.websiteUrl` during conversion; the
        // detail modal falls back to constructing the canonical CurseForge URL
        // from `slug`. Keep `website_url` None for v1.
        let summary = convert_mod_summary(m);

        // Second hop: CurseForge serves the long description as HTML from a
        // dedicated endpoint. A failure here degrades to an empty body (the
        // modal falls back to the short summary) rather than failing the
        // whole detail load.
        let desc_url = format!("{}/v1/mods/{}/description", self.base, project_id);
        let body_html =
            match crate::network::request::get(&desc_url, &[("x-api-key", auth)], "mods").await {
                Ok(r) if (200..300).contains(&r.status) => {
                    match serde_json::from_slice::<types::Envelope<String>>(&r.body) {
                        Ok(e) => crate::mods::render::sanitize_html(&e.data),
                        Err(_) => String::new(),
                    }
                }
                _ => String::new(),
            };

        Ok(ModProject {
            summary,
            body_html,
            gallery,
            website_url: None,
        })
    }

    async fn versions(
        &self,
        project_id: &str,
        mc: Option<&str>,
        loader: Option<LoaderKind>,
    ) -> Result<Vec<ModVersion>, Error> {
        let auth = self.auth()?;
        let mut params: Vec<(&str, String)> = vec![("pageSize", "50".to_string())];
        if let Some(v) = mc {
            params.push(("gameVersion", v.to_string()));
        }
        if let Some(l) = loader {
            if l != LoaderKind::Vanilla {
                params.push(("modLoaderType", types::loader_type(l).to_string()));
            }
        }
        let url = format!(
            "{}/v1/mods/{}/files?{}",
            self.base,
            project_id,
            encode_pairs(&params)
        );
        let resp = crate::network::request::get(&url, &[("x-api-key", auth)], "mods")
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
        let env: types::ListEnvelope<types::File> = self.map_status(resp, url)?;
        let versions: Vec<ModVersion> = env
            .data
            .into_iter()
            .filter_map(|f| convert_version(f, project_id))
            // CF's `modLoaderType` query filter is unreliable and leaks files
            // for other loaders (e.g. a NeoForge build returned for a Forge
            // query). Post-filter on the loader tags we parsed from
            // `gameVersions`: drop any version that names loaders but not the
            // requested one. A version with no detectable loader tag is
            // ambiguous and kept. Forge and NeoForge are distinct here.
            .filter(|v| match loader {
                Some(want) if !v.loaders.is_empty() => v.loaders.contains(&want),
                _ => true,
            })
            .collect();
        // Second, filename-based guard against loader mis-tagging (shared with
        // the Modrinth client).
        Ok(crate::mods::platform::drop_filename_loader_mismatches(
            versions, loader,
        ))
    }

    async fn datapack_versions(
        &self,
        project_id: &str,
        mc_version: Option<&str>,
    ) -> Result<Vec<ModVersion>, Error> {
        // A CurseForge project belongs to exactly one class, so a datapack
        // project's (class 6945, wired into `class_id` for search) files are
        // all datapack zips: unlike Modrinth's hybrid projects there is no mod
        // jar to filter out, and no `modLoaderType` value could express the
        // kind anyway — which is why this sat on the empty trait default (with
        // the source hidden in the UI) until this PR. `gameVersion` is the
        // only filter.
        let auth = self.auth()?;
        let mut params: Vec<(&str, String)> = vec![("pageSize", "50".to_string())];
        if let Some(v) = mc_version {
            params.push(("gameVersion", v.to_string()));
        }
        let url = format!(
            "{}/v1/mods/{}/files?{}",
            self.base,
            project_id,
            encode_pairs(&params)
        );
        let resp = crate::network::request::get(&url, &[("x-api-key", auth)], "mods")
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
        let env: types::ListEnvelope<types::File> = self.map_status(resp, url)?;
        let mut versions: Vec<ModVersion> = env
            .data
            .into_iter()
            .filter_map(|f| convert_version(f, project_id))
            .collect();
        // Same rule as the Modrinth datapack listing: a datapack build has no
        // meaningful Java-loader identity, and any tag `convert_version`
        // parsed out of a hybrid-tagged `gameVersions` would be misleading on
        // a datapack row. The Java loader post-filter and the
        // filename-mismatch defence are skipped for the same reason.
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
            // The CF dep pin handle is the numeric `file_id`. A `ModVersion`'s
            // `version_id` is the stringified file id (see `convert_version`),
            // so the pin is matched by passing `file_id.to_string()` as the
            // pin handle to `select_dep_version`.
            let (pid, pin) = match &dep.project_ref {
                DepProjectRef::Curseforge { mod_id, file_id } => {
                    (mod_id.to_string(), file_id.map(|f| f.to_string()))
                }
                DepProjectRef::Modrinth { .. } => {
                    // Cross-source dep we can't resolve here — only flag if required.
                    if dep.kind == DepKind::Required {
                        unresolvable.push(dep.project_ref.clone());
                    }
                    continue;
                }
            };
            let vs = self.versions(&pid, Some(mc), Some(loader)).await?;
            // Actually seek the pinned file rather than labelling newest as a
            // pin. CF dependency metadata carries no version range — pin only.
            // `PinHonored` only when the pinned file is among the compatible
            // builds; `FellBackFromPin` when a file_id was given but absent;
            // `NewestNoPin` when no file_id.
            if let Some((i, reason)) =
                crate::mods::dep_select::select_dep_version(&vs, pin.as_deref(), None)
            {
                let v = vs
                    .into_iter()
                    .nth(i)
                    .expect("index returned by select_dep_version is in range");
                let resolved = ResolvedDep {
                    project_ref: dep.project_ref.clone(),
                    version: v,
                    selection_reason: reason,
                };
                match dep.kind {
                    DepKind::Required => required.push(resolved),
                    DepKind::Optional => optional.push(resolved),
                    _ => {}
                }
            } else if dep.kind == DepKind::Required {
                // Only a missing *required* dep is worth surfacing; an optional
                // one that has no compatible build is skipped silently.
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
        // CF mod ids are numeric; skip anything that isn't. An empty set makes
        // no request and needs no key.
        let mod_ids: Vec<u32> = ids.iter().filter_map(|s| s.parse::<u32>().ok()).collect();
        if mod_ids.is_empty() {
            return Ok(Vec::new());
        }
        let auth = self.auth()?;
        let mut out = Vec::with_capacity(mod_ids.len());
        for chunk in mod_ids.chunks(BATCH_CHUNK) {
            // CF: POST /v1/mods  {"modIds":[…]} → {"data":[Mod,…]}; unknown omitted.
            let url = format!("{}/v1/mods", self.base);
            // Serialising {"modIds": [u32, …]} is infallible.
            let body = serde_json::to_vec(&serde_json::json!({ "modIds": chunk }))
                .expect("a JSON object of numeric ids always serializes");
            let resp = crate::network::request::post(
                &url,
                &[
                    ("x-api-key", auth),
                    ("content-type", "application/json"),
                    ("accept", "application/json"),
                ],
                &body,
                "mods",
            )
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
            let env: types::ListEnvelope<types::Mod> = self.map_status(resp, url)?;
            out.extend(env.data.into_iter().map(convert_mod_summary));
        }
        Ok(out)
    }

    async fn versions_by_ids(&self, version_ids: &[&str]) -> Result<Vec<ModVersion>, Error> {
        // A CF "version id" is the file id (numeric). Skip non-numeric ids.
        let file_ids: Vec<u32> = version_ids
            .iter()
            .filter_map(|s| s.parse::<u32>().ok())
            .collect();
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }
        let auth = self.auth()?;
        let mut out = Vec::with_capacity(file_ids.len());
        for chunk in file_ids.chunks(BATCH_CHUNK) {
            // CF: POST /v1/mods/files  {"fileIds":[…]} → {"data":[File,…]}, each
            // file carrying its `dependencies` and parent `modId`.
            let url = format!("{}/v1/mods/files", self.base);
            // Serialising {"fileIds": [u32, …]} is infallible.
            let body = serde_json::to_vec(&serde_json::json!({ "fileIds": chunk }))
                .expect("a JSON object of numeric ids always serializes");
            let resp = crate::network::request::post(
                &url,
                &[
                    ("x-api-key", auth),
                    ("content-type", "application/json"),
                    ("accept", "application/json"),
                ],
                &body,
                "mods",
            )
            .await
            .map_err(|e| Error::mods_network(url.clone(), e))?;
            let env: types::ListEnvelope<types::File> = self.map_status(resp, url)?;
            out.extend(env.data.into_iter().filter_map(|f| {
                let pid = f.mod_id.to_string();
                convert_version(f, &pid)
            }));
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
        use futures_util::stream::{self, StreamExt};

        let auth = self.auth()?.to_string();
        // Reuse the normalized file list (version_id == CF file id, published_at
        // set), fetched unfiltered so the target is always present.
        let mut versions = self.versions(project_id, None, None).await?;
        versions.sort_by(|a, b| b.published_at.cmp(&a.published_at)); // newest-first

        // Restrict to the TARGET's release lineage — files sharing at least one
        // loader AND one MC version with the target — so a cross-loader / cross-MC
        // build published between `base` and `target` by date is not pulled into
        // the window. The target defines the lineage, so it is always present.
        let (t_mcs, t_loaders): (Vec<String>, Vec<LoaderKind>) = versions
            .iter()
            .find(|v| v.version_id == target_version_id)
            .map(|t| (t.mc_versions.clone(), t.loaders.clone()))
            .unwrap_or_default();
        let lineage: Vec<crate::mods::platform::ModVersion> =
            if t_mcs.is_empty() && t_loaders.is_empty() {
                versions
            } else {
                versions
                    .into_iter()
                    .filter(|v| {
                        v.loaders.iter().any(|l| t_loaders.contains(l))
                            && v.mc_versions.iter().any(|g| t_mcs.contains(g))
                    })
                    .collect()
            };

        let ids: Vec<&str> = lineage.iter().map(|v| v.version_id.as_str()).collect();
        let (start, end, full) = changelog_window(&ids, target_version_id, base_version_id);
        let window: Vec<crate::mods::platform::ModVersion> = lineage[start..end].to_vec();

        // CurseForge has no changelog in the file list — it is a per-file
        // endpoint. Fan out but preserve newest-first order (`buffered`, not
        // `buffer_unordered`), bounded to avoid a rate-limit burst.
        const CHANGELOG_CONCURRENCY: usize = 6;
        let base = self.base.clone();
        let sections: Vec<ChangelogSection> = stream::iter(window.into_iter())
            .map(|v| {
                let base = base.clone();
                let auth = auth.clone();
                let pid = project_id.to_string();
                async move {
                    let url = format!("{}/v1/mods/{}/files/{}/changelog", base, pid, v.version_id);
                    let body_html = match crate::network::request::get(
                        &url,
                        &[("x-api-key", auth.as_str())],
                        "mods",
                    )
                    .await
                    {
                        Ok(r) if (200..300).contains(&r.status) => {
                            serde_json::from_slice::<types::Envelope<String>>(&r.body)
                                .map(|e| crate::mods::render::sanitize_html(&e.data))
                                .unwrap_or_default()
                        }
                        _ => String::new(),
                    };
                    ChangelogSection {
                        version_id: v.version_id.clone(),
                        version_number: v.version_number.clone(),
                        published_at: v.published_at.clone(),
                        body_html,
                    }
                }
            })
            .buffered(CHANGELOG_CONCURRENCY)
            .collect()
            .await;

        let truncated = (end - start < full).then_some(full as u32);
        Ok(ChangelogResult {
            sections,
            truncated,
        })
    }
}

/// Loader families a CurseForge project is *known* to support, derived from
/// `latestFilesIndexes`.
///
/// Returns `None` for UNKNOWN — callers must never treat that as "supports
/// nothing". Never returns `Some(empty)`.
///
/// The index itself is a full-history union (verified by exhaustive
/// pagination), so on the game-version axis it can only over-state support,
/// which is the safe direction. The hazard is the *loader* axis: `modLoader` is
/// absent on pre-tagging-era files — 30 of JEI's 105 entries, 26 of Bookshelf's
/// 132 — and those untagged files can belong to any loader. Collecting only the
/// tagged entries would therefore derive `[Forge]` for a project whose Fabric
/// builds happen to be untagged, and the dependency graph would then silently
/// hide a genuinely missing dependency. So a single untagged entry disqualifies
/// the whole project.
///
/// `modLoader: 0` is the documented "Any" wildcard and is disqualifying for the
/// same reason. Measured effect of this conservatism: 7 of 14 popular
/// dependency libraries stay derivable, including Fabric API — the single most
/// common phantom dependency.
fn project_loaders(idx: &[types::FileIndex]) -> Option<Vec<LoaderKind>> {
    // Any untagged or wildcard entry ⇒ the tagging is provably incomplete.
    if idx.iter().any(|e| matches!(e.mod_loader, None | Some(0))) {
        return None;
    }
    let mut out: Vec<LoaderKind> = Vec::new();
    for e in idx {
        // 2 = Cauldron, 3 = LiteLoader: real tags, but never an instance loader
        // here, so their presence cannot change a disjointness test.
        let kind = match e.mod_loader {
            Some(1) => LoaderKind::Forge,
            Some(4) => LoaderKind::Fabric,
            Some(5) => LoaderKind::Quilt,
            Some(6) => LoaderKind::NeoForge,
            _ => continue,
        };
        if !out.contains(&kind) {
            out.push(kind);
        }
    }
    // An empty set is never authoritative (no entries at all, or only
    // Cauldron/LiteLoader) — report unknown rather than "supports nothing".
    (!out.is_empty()).then_some(out)
}

fn convert_mod_summary(m: types::Mod) -> ModSummary {
    let loaders = project_loaders(&m.latest_files_indexes);
    ModSummary {
        source: ModSource::Curseforge,
        project_id: m.id.to_string(),
        slug: Some(m.slug),
        name: m.name,
        summary: m.summary,
        icon_url: m.logo.and_then(|l| l.url),
        downloads: m.download_count as f64,
        author: m
            .authors
            .into_iter()
            .map(|a| a.name)
            .next()
            .unwrap_or_default(),
        updated_at: m.date_modified,
        loaders,
    }
}

fn convert_version(f: types::File, project_id: &str) -> Option<ModVersion> {
    let sha1 = f
        .hashes
        .iter()
        .find(|h| h.algo == 1)
        .map(|h| h.value.clone());
    let url = f.download_url.unwrap_or_default();
    let distribution_allowed = !url.is_empty() && f.is_available;
    // CF mixes Minecraft versions and loader names in `gameVersions`
    // (e.g. ["1.20.1", "NeoForge"]). Split them: recognized loader tags
    // populate `loaders`; everything else stays in `mc_versions`. This is
    // what lets `versions()` post-filter by loader, since CF's own
    // `modLoaderType` query filter is unreliable.
    let mut loaders = Vec::new();
    let mut mc_versions = Vec::new();
    for gv in f.game_versions {
        match types::loader_from_tag(&gv) {
            Some(l) if !loaders.contains(&l) => loaders.push(l),
            Some(_) => {} // duplicate loader tag
            // Drop "Client"/"Server" markers; keep everything else as an MC version.
            None if types::is_environment_marker(&gv) => {}
            None => mc_versions.push(gv),
        }
    }
    Some(ModVersion {
        source: ModSource::Curseforge,
        project_id: project_id.to_string(),
        version_id: f.id.to_string(),
        name: f.display_name,
        version_number: f.file_name.clone(),
        mc_versions,
        loaders,
        primary_file: ModFile {
            filename: f.file_name,
            url,
            sha1,
            size: f.file_length as f64,
            distribution_allowed,
            sha256: None,
        },
        deps: f
            .dependencies
            .into_iter()
            .filter_map(|d| {
                let kind = match d.relation_type {
                    3 => DepKind::Required,
                    2 => DepKind::Optional,
                    5 => DepKind::Incompatible,
                    1 => DepKind::Embedded,
                    _ => return None,
                };
                Some(ModDepLink {
                    kind,
                    project_ref: DepProjectRef::Curseforge {
                        mod_id: d.mod_id,
                        file_id: None,
                    },
                })
            })
            .collect(),
        published_at: f.file_date,
    })
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

fn encode_pairs(pairs: &[(&str, String)]) -> String {
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        out.push_str(&urlencode(k));
        out.push('=');
        out.push_str(&urlencode(v));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    fn client(uri: String) -> CurseForgeClient {
        CurseForgeClient::with_base_and_key(uri, Some("test-key".into()))
    }

    /// Build a `latestFilesIndexes` shape from `(gameVersion, modLoader)` pairs.
    fn idx(pairs: &[(&str, Option<u8>)]) -> Vec<types::FileIndex> {
        pairs
            .iter()
            .map(|(gv, ml)| types::FileIndex {
                game_version: (*gv).to_string(),
                mod_loader: *ml,
            })
            .collect()
    }

    /// Fully tagged index ⇒ derivable. Sodium's real shape: it has never
    /// shipped a Forge build, so this doubles as a positive control that the
    /// derivation does not fabricate loaders it never saw.
    #[test]
    fn project_loaders_derives_from_a_fully_tagged_index() {
        let got = project_loaders(&idx(&[
            ("1.21.1", Some(4)),
            ("1.21.1", Some(5)),
            ("1.21.1", Some(6)),
            ("1.20.1", Some(4)),
        ]));
        let got = got.expect("a fully tagged index is derivable");
        assert!(got.contains(&LoaderKind::Fabric));
        assert!(got.contains(&LoaderKind::Quilt));
        assert!(got.contains(&LoaderKind::NeoForge));
        assert!(
            !got.contains(&LoaderKind::Forge),
            "Sodium ships no Forge build — the derivation must not invent one"
        );
    }

    /// A single untagged entry disqualifies the whole project. Those files
    /// predate CurseForge's loader tagging and can belong to ANY loader, so
    /// deriving from the tagged remainder could hide a real dependency.
    /// JEI's real index is 30 untagged of 105.
    #[test]
    fn project_loaders_is_unknown_when_any_entry_is_untagged() {
        assert_eq!(
            project_loaders(&idx(&[
                ("1.8.9", None),
                ("1.21.1", Some(1)),
                ("1.21.1", Some(4)),
            ])),
            None
        );
        // All untagged (a project that stopped publishing before tagging).
        assert_eq!(project_loaders(&idx(&[("1.2.5", None)])), None);
    }

    /// `modLoader: 0` is the documented "Any" wildcard — an unbounded set, so
    /// it cannot license a suppression either.
    #[test]
    fn project_loaders_is_unknown_for_the_any_wildcard() {
        assert_eq!(
            project_loaders(&idx(&[("1.21.1", Some(0)), ("1.21.1", Some(6))])),
            None
        );
    }

    /// Degenerate inputs must never read as "supports nothing".
    #[test]
    fn project_loaders_is_unknown_for_empty_and_unmappable_indexes() {
        assert_eq!(project_loaders(&[]), None, "no index at all");
        assert_eq!(
            project_loaders(&idx(&[("1.6.4", Some(2)), ("1.6.4", Some(3))])),
            None,
            "only Cauldron/LiteLoader — tagged, but never an instance loader"
        );
    }

    /// The field is absent on older/drifted responses; that must decode to an
    /// empty index rather than failing the whole batch.
    #[test]
    fn mod_decodes_without_latest_files_indexes() {
        let m: types::Mod = serde_json::from_str(
            r#"{"id":1,"slug":"s","name":"N","summary":"","downloadCount":0,
                "authors":[],"logo":null,"dateModified":null,"links":{}}"#,
        )
        .expect("an absent latestFilesIndexes must not be a decode error");
        assert!(m.latest_files_indexes.is_empty());
        assert_eq!(project_loaders(&m.latest_files_indexes), None);
    }

    #[tokio::test]
    async fn summaries_batches_mods_and_skips_non_numeric() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/mods"))
            .and(header("x-api-key", "test-key"))
            .and(wiremock::matchers::body_json(
                serde_json::json!({ "modIds": [10, 20] }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"id":10,"slug":"jei","name":"JEI","summary":"items","downloadCount":5,
                     "dateModified":null,"authors":[{"name":"m"}],
                     "logo":{"url":"https://example/i.png"},"links":{"websiteUrl":null}},
                    {"id":20,"slug":"sodium","name":"Sodium","summary":"perf","downloadCount":9,
                     "dateModified":null,"authors":[{"name":"jelly"}],
                     "logo":null,"links":{"websiteUrl":null}}
                ]
            })))
            .mount(&s)
            .await;
        let c = client(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // "abc" is non-numeric and must be dropped before the request is built.
        let out = c.summaries(&["10", "abc", "20"]).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "JEI");
        assert_eq!(out[0].project_id, "10");
        assert_eq!(out[1].author, "jelly");
    }

    #[tokio::test]
    async fn summaries_all_non_numeric_makes_no_request() {
        let _g = test_lock();
        // No mock mounted — a request would 404 and the call would error.
        let s = MockServer::start().await;
        let c = client(s.uri());
        let out = c.summaries(&["abc", "xyz"]).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn versions_by_ids_parses_files_with_deps() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .and(header("x-api-key", "test-key"))
            .and(wiremock::matchers::body_json(
                serde_json::json!({ "fileIds": [555] }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"id":555,"modId":42,"displayName":"JEI 15","fileName":"jei-15.jar",
                     "fileLength":100,"hashes":[{"value":"abc","algo":1}],
                     "gameVersions":["1.20.1","Fabric"],"downloadUrl":"https://cdn/jei.jar",
                     "fileDate":"2026-05-01T00:00:00Z","isAvailable":true,"releaseType":1,
                     "dependencies":[{"modId":99,"relationType":3}]}
                ]
            })))
            .mount(&s)
            .await;
        let c = client(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let out = c.versions_by_ids(&["555"]).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].project_id, "42"); // derived from the file's modId
        assert_eq!(out[0].version_id, "555");
        assert_eq!(out[0].deps.len(), 1);
        assert_eq!(out[0].deps[0].kind, DepKind::Required);
    }

    #[tokio::test]
    async fn changelog_range_windows_files_and_fetches_per_file_html() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/42/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"id":300,"modId":42,"displayName":"0.6.0","fileName":"mod-0.6.0.jar",
                     "fileLength":100,"hashes":[{"value":"a","algo":1}],
                     "gameVersions":["1.21.4","Fabric"],"downloadUrl":"https://cdn/3.jar",
                     "fileDate":"2026-06-03T00:00:00Z","isAvailable":true,"releaseType":1,"dependencies":[]},
                    {"id":200,"modId":42,"displayName":"0.5.9","fileName":"mod-0.5.9.jar",
                     "fileLength":100,"hashes":[{"value":"b","algo":1}],
                     "gameVersions":["1.21.4","Fabric"],"downloadUrl":"https://cdn/2.jar",
                     "fileDate":"2026-05-01T00:00:00Z","isAvailable":true,"releaseType":1,"dependencies":[]},
                    {"id":100,"modId":42,"displayName":"0.5.8","fileName":"mod-0.5.8.jar",
                     "fileLength":100,"hashes":[{"value":"c","algo":1}],
                     "gameVersions":["1.21.4","Fabric"],"downloadUrl":"https://cdn/1.jar",
                     "fileDate":"2026-04-01T00:00:00Z","isAvailable":true,"releaseType":1,"dependencies":[]}
                ]
            })))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/42/files/300/changelog"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"data":"<p>new stuff</p><script>bad()</script>"}),
            ))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/42/files/200/changelog"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data":"<p>middle</p>"})),
            )
            .mount(&s)
            .await;
        let c = client(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // installed file 100, target file 300 → sections for 300 and 200.
        let res = c.changelog_range("42", "300", Some("100")).await.unwrap();
        assert_eq!(res.sections.len(), 2);
        assert_eq!(res.sections[0].version_id, "300");
        assert!(res.sections[0].body_html.contains("new stuff"));
        assert!(
            !res.sections[0].body_html.contains("<script"),
            "script stripped by sanitizer"
        );
        assert_eq!(res.sections[1].version_id, "200");
        assert_eq!(res.truncated, None);
    }

    #[tokio::test]
    async fn files_by_fingerprint_maps_fingerprint_to_identity() {
        // The echoed `fileFingerprint` — NOT the top-level `id` (which is the
        // modId) — is what keys each match back to the sha1 we sent.
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/fingerprints"))
            .and(header("x-api-key", "test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "exactMatches": [
                    {
                        "id": 42,
                        "file": {
                            "id": 555,
                            "modId": 42,
                            "fileFingerprint": 111,
                            "displayName": "JEI 15.0.0"
                        }
                    }
                ]}
            })))
            .mount(&s)
            .await;
        let c = client(s.uri());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        // Uppercase sha in: the returned map key is lowercased.
        let out = c
            .files_by_fingerprint(&[(111u32, "SHA-A".to_string())])
            .await
            .unwrap();
        let hit = out
            .get("sha-a")
            .expect("fingerprint should map back to its sha1");
        assert_eq!(hit.project_id, "42");
        assert_eq!(hit.version_id, "555");
        assert_eq!(hit.version_number.as_deref(), Some("JEI 15.0.0"));
    }

    #[tokio::test]
    async fn missing_key_returns_platform_auth_missing() {
        let _g = test_lock();
        let s = MockServer::start().await;
        let c = CurseForgeClient::with_base_and_key(s.uri(), None);
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
            kind: ContentKind::Mod,
            query: "x".into(),
            mc_version: None,
            loader: None,
            sort: ModSort::Relevance,
            page_size: 20,
            offset: 0,
            plugin_core: None,
        };
        let err = c.search(&q).await.unwrap_err();
        match err {
            Error::ModsPlatformAuth { kind } => {
                assert_eq!(kind, crate::error::ModsAuthKind::Missing)
            }
            _ => panic!("expected ModsPlatformAuth::Missing, got {err:?}"),
        }
    }

    #[tokio::test]
    async fn unauthorized_clears_key_and_returns_invalid() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(header("x-api-key", "test-key"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&s)
            .await;
        let c = client(s.uri());
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
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
        match err {
            Error::ModsPlatformAuth { kind } => {
                assert_eq!(kind, crate::error::ModsAuthKind::Invalid)
            }
            _ => panic!("expected ModsPlatformAuth::Invalid, got {err:?}"),
        }
    }

    #[tokio::test]
    async fn project_maps_screenshots_and_sanitizes_description() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/77"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "id": 77, "slug": "jei", "name": "JEI", "summary": "items",
                    "downloadCount": 5, "authors": [{"name":"m"}],
                    "logo": {"url":"https://example/icon.png"},
                    "links": {"websiteUrl": null},
                    "screenshots": [{"url":"https://media.forgecdn.net/s.png","title":"Shot"}]
                }
            })))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/77/description"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": "<p>Hi</p><script>alert(1)</script>"
            })))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let c = client(s.uri());
        let p = c.project("77").await.unwrap();
        assert_eq!(p.gallery.len(), 1);
        assert_eq!(p.gallery[0].url, "https://media.forgecdn.net/s.png");
        assert!(p.body_html.contains("<p>Hi</p>"));
        assert!(!p.body_html.contains("<script"));
    }

    #[tokio::test]
    async fn search_parses_envelope() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                "data":[{"id":12345,"slug":"jei","name":"JEI","summary":"Items",
                         "downloadCount":1234,"authors":[{"name":"mezz"}],
                         "logo":{"url":"https://example/icon.png"},
                         "dateModified":"2026-05-01T00:00:00Z",
                         "links":{"websiteUrl":"https://www.curseforge.com/minecraft/mc-mods/jei"}}],
                "pagination":{"index":0,"pageSize":20,"resultCount":1,"totalCount":1}
            }"#,
            ))
            .mount(&s)
            .await;
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
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
        let page = client(s.uri()).search(&q).await.unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.hits[0].name, "JEI");
        assert_eq!(page.hits[0].project_id, "12345");
    }

    #[tokio::test]
    async fn datapack_versions_list_by_game_version_with_no_loader_semantics() {
        // A CurseForge project belongs to exactly ONE class, so a datapack
        // project's (class 6945) files are all datapack zips — the reason no
        // modLoaderType filter exists or is sent. The mock matches the exact
        // query: a gameVersion param and nothing else, so an implementation
        // that sneaks a modLoaderType in fails to match and the test fails.
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/999/files"))
            .and(query_param("gameVersion", "1.21.4"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                "data":[{"id":7,"modId":999,"displayName":"Terralith v2.5.5",
                         "fileName":"Terralith_1.21_v2.5.5.zip",
                         "fileLength":1000,"hashes":[{"value":"aa","algo":1}],
                         "gameVersions":["1.21.4","Fabric","Forge"],
                         "downloadUrl":"https://cdn/terralith.zip",
                         "fileDate":"2026-06-01T00:00:00Z","isAvailable":true,
                         "releaseType":1,"dependencies":[]}],
                "pagination":null}"#,
            ))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let v = client(s.uri())
            .datapack_versions("999", Some("1.21.4"))
            .await
            .unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].primary_file.filename, "Terralith_1.21_v2.5.5.zip");
        // A hybrid-tagged file's Java-loader tags must not leak onto a
        // datapack build — same rule as the Modrinth datapack listing.
        assert!(
            v[0].loaders.is_empty(),
            "loader tags must be cleared: {:?}",
            v[0].loaders
        );
    }

    #[tokio::test]
    async fn versions_marks_distribution_disabled_when_url_absent() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/12345/files"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                "data":[{"id":99,"modId":12345,"displayName":"v1.0","fileName":"x.jar",
                         "fileLength":100,"hashes":[{"value":"abc","algo":1}],
                         "gameVersions":["1.20.1"],"downloadUrl":null,
                         "fileDate":"2026-05-01T00:00:00Z","isAvailable":true,
                         "releaseType":1,"dependencies":[]}],
                "pagination":null}"#,
            ))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let v = client(s.uri())
            .versions("12345", Some("1.20.1"), Some(LoaderKind::Fabric))
            .await
            .unwrap();
        assert_eq!(v.len(), 1);
        assert!(!v[0].primary_file.distribution_allowed);
    }

    #[test]
    fn convert_version_splits_loaders_from_mc_versions() {
        let f = types::File {
            id: 99,
            mod_id: 12345,
            display_name: "Xaero 1.20.1 NeoForge".into(),
            file_name: "xaerominimap.jar".into(),
            file_length: 100,
            hashes: vec![types::Hash {
                value: "abc".into(),
                algo: 1,
            }],
            game_versions: vec!["1.20.1".into(), "NeoForge".into(), "Client".into()],
            download_url: Some("https://example/x.jar".into()),
            file_date: None,
            is_available: true,
            release_type: 1,
            dependencies: vec![],
        };
        let v = convert_version(f, "12345").unwrap();
        assert_eq!(v.loaders, vec![LoaderKind::NeoForge]);
        // Loader tags and environment markers ("Client"/"Server") are stripped;
        // only real MC versions remain in mc_versions.
        assert_eq!(v.mc_versions, vec!["1.20.1".to_string()]);
    }

    /// A required CF dep whose ref pins a `file_id` that IS among the
    /// compatible builds must select that exact file and report `PinHonored`.
    #[tokio::test]
    async fn resolve_deps_honors_pinned_file_id() {
        let s = MockServer::start().await;
        // Dep mod 42 has two compatible Fabric builds; the pin (file 555) is the
        // OLDER one. CF returns newest-first; the pin must override the default.
        Mock::given(method("GET"))
            .and(path("/v1/mods/42/files"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                "data":[
                  {"id":777,"modId":42,"displayName":"new","fileName":"dep-new.jar",
                   "fileLength":100,"hashes":[{"value":"a","algo":1}],
                   "gameVersions":["1.20.1","Fabric"],"downloadUrl":"https://e/new.jar",
                   "fileDate":"2026-05-01T00:00:00Z","isAvailable":true,"releaseType":1,"dependencies":[]},
                  {"id":555,"modId":42,"displayName":"old","fileName":"dep-old.jar",
                   "fileLength":100,"hashes":[{"value":"b","algo":1}],
                   "gameVersions":["1.20.1","Fabric"],"downloadUrl":"https://e/old.jar",
                   "fileDate":"2026-01-01T00:00:00Z","isAvailable":true,"releaseType":1,"dependencies":[]}
                ],
                "pagination":null}"#,
            ))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let primary = ModVersion {
            source: ModSource::Curseforge,
            project_id: "1".into(),
            version_id: "100".into(),
            name: "Primary".into(),
            version_number: "primary.jar".into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![LoaderKind::Fabric],
            primary_file: ModFile {
                filename: "primary.jar".into(),
                url: "https://e/p.jar".into(),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: vec![ModDepLink {
                kind: DepKind::Required,
                project_ref: DepProjectRef::Curseforge {
                    mod_id: 42,
                    file_id: Some(555),
                },
            }],
            published_at: None,
        };
        let rd = client(s.uri())
            .resolve_deps(&primary, "1.20.1", LoaderKind::Fabric)
            .await
            .unwrap();
        assert_eq!(rd.required.len(), 1);
        assert_eq!(rd.required[0].version.version_id, "555");
        assert_eq!(
            rd.required[0].selection_reason,
            crate::mods::platform::SelectionReason::PinHonored
        );
    }

    /// A pinned `file_id` that is NOT among the compatible builds falls back to
    /// the newest compatible build and reports `FellBackFromPin`.
    #[tokio::test]
    async fn resolve_deps_falls_back_when_pinned_file_absent() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/42/files"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                "data":[
                  {"id":777,"modId":42,"displayName":"new","fileName":"dep-new.jar",
                   "fileLength":100,"hashes":[{"value":"a","algo":1}],
                   "gameVersions":["1.20.1","Fabric"],"downloadUrl":"https://e/new.jar",
                   "fileDate":"2026-05-01T00:00:00Z","isAvailable":true,"releaseType":1,"dependencies":[]}
                ],
                "pagination":null}"#,
            ))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let primary = ModVersion {
            source: ModSource::Curseforge,
            project_id: "1".into(),
            version_id: "100".into(),
            name: "Primary".into(),
            version_number: "primary.jar".into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec![LoaderKind::Fabric],
            primary_file: ModFile {
                filename: "primary.jar".into(),
                url: "https://e/p.jar".into(),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: vec![ModDepLink {
                kind: DepKind::Required,
                project_ref: DepProjectRef::Curseforge {
                    mod_id: 42,
                    // 999 is not among the compatible builds (only 777 is).
                    file_id: Some(999),
                },
            }],
            published_at: None,
        };
        let rd = client(s.uri())
            .resolve_deps(&primary, "1.20.1", LoaderKind::Fabric)
            .await
            .unwrap();
        assert_eq!(rd.required.len(), 1);
        assert_eq!(rd.required[0].version.version_id, "777");
        assert_eq!(
            rd.required[0].selection_reason,
            crate::mods::platform::SelectionReason::FellBackFromPin
        );
    }

    #[tokio::test]
    async fn versions_filters_out_mismatched_loader() {
        // CF leaks a NeoForge file even when modLoaderType=Forge is requested.
        // The post-filter must drop it and keep only the Forge file.
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/12345/files"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                "data":[
                  {"id":1,"modId":12345,"displayName":"neo","fileName":"x-neoforge.jar",
                   "fileLength":100,"hashes":[{"value":"a","algo":1}],
                   "gameVersions":["1.20.1","NeoForge"],"downloadUrl":"https://e/n.jar",
                   "fileDate":null,"isAvailable":true,"releaseType":1,"dependencies":[]},
                  {"id":2,"modId":12345,"displayName":"forge","fileName":"x-forge.jar",
                   "fileLength":100,"hashes":[{"value":"b","algo":1}],
                   "gameVersions":["1.20.1","Forge"],"downloadUrl":"https://e/f.jar",
                   "fileDate":null,"isAvailable":true,"releaseType":1,"dependencies":[]}
                ],
                "pagination":null}"#,
            ))
            .mount(&s)
            .await;
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let v = client(s.uri())
            .versions("12345", Some("1.20.1"), Some(LoaderKind::Forge))
            .await
            .unwrap();
        assert_eq!(v.len(), 1, "only the Forge file should survive");
        assert_eq!(v[0].loaders, vec![LoaderKind::Forge]);
        assert_eq!(v[0].version_number, "x-forge.jar");
    }

    /// Build `count` minimal CF mod JSON entries with ids `start..start+count`.
    fn mod_entries(start: u32, count: u32) -> Vec<serde_json::Value> {
        (start..start + count)
            .map(|i| {
                serde_json::json!({
                    "id": i, "slug": format!("m{i}"), "name": format!("Mod {i}"),
                    "summary": "s", "downloadCount": 1, "authors": [{"name": "a"}],
                    "logo": {"url": "https://example/i.png"},
                    "links": {"websiteUrl": null}
                })
            })
            .collect()
    }

    #[tokio::test]
    async fn search_chunks_page_size_above_cf_max() {
        // CurseForge rejects pageSize > 50 with HTTP 400. A UI request for 100
        // results/page must fan out into two ≤50 windows (index 0 + 50) and
        // stitch them — never sending pageSize=100. The mocks only match
        // pageSize=50, so a single pageSize=100 request would 404 and fail.
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(query_param("pageSize", "50"))
            .and(query_param("index", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": mod_entries(0, 50),
                "pagination": {"index": 0, "pageSize": 50, "resultCount": 50, "totalCount": 120}
            })))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(query_param("pageSize", "50"))
            .and(query_param("index", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": mod_entries(50, 50),
                "pagination": {"index": 50, "pageSize": 50, "resultCount": 50, "totalCount": 120}
            })))
            .mount(&s)
            .await;
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
            kind: ContentKind::Mod,
            query: String::new(),
            mc_version: None,
            loader: None,
            sort: ModSort::Downloads,
            page_size: 100,
            offset: 0,
            plugin_core: None,
        };
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = client(s.uri()).search(&q).await.unwrap();
        assert_eq!(page.hits.len(), 100, "two 50-windows should stitch to 100");
        assert_eq!(page.total, 120);
        assert_eq!(page.page_size, 100, "returns the requested page size");
        assert_eq!(page.offset, 0);
        assert_eq!(page.hits[0].project_id, "0");
        assert_eq!(page.hits[99].project_id, "99");
    }

    #[tokio::test]
    async fn search_stops_early_when_window_underfills() {
        // Only 30 results exist though 100 were requested: the first window
        // returns < pageSize, so no second request is made.
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(query_param("index", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": mod_entries(0, 30),
                "pagination": {"index": 0, "pageSize": 50, "resultCount": 30, "totalCount": 30}
            })))
            .mount(&s)
            .await;
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
            kind: ContentKind::Mod,
            query: String::new(),
            mc_version: None,
            loader: None,
            sort: ModSort::Relevance,
            page_size: 100,
            offset: 0,
            plugin_core: None,
        };
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = client(s.uri()).search(&q).await.unwrap();
        assert_eq!(page.hits.len(), 30);
        assert_eq!(page.total, 30);
    }

    #[tokio::test]
    async fn search_sends_class_id_for_shader_kind() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/search"))
            .and(header("x-api-key", "test-key"))
            .and(query_param("classId", "6552"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [],
                "pagination": {"index": 0, "pageSize": 20, "resultCount": 0, "totalCount": 0}
            })))
            .mount(&s)
            .await;
        let q = ModSearchQuery {
            source: ModSource::Curseforge,
            kind: ContentKind::Shader,
            query: "complementary".into(),
            mc_version: None,
            loader: None,
            sort: ModSort::Relevance,
            page_size: 20,
            offset: 0,
            plugin_core: None,
        };
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let page = client(s.uri()).search(&q).await.unwrap();
        assert_eq!(page.total, 0);
        assert!(page.hits.is_empty());
    }
}

/// Outcome of validating a candidate CurseForge API key against the
/// `/v1/games/432` ping. Pure classification of an already-received
/// response — transport failures are handled by the caller before this.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeyCheckOutcome {
    /// 2xx — key accepted.
    Ok,
    /// The API genuinely rejected the key (non-2xx with a real API body).
    Invalid,
    /// Reached the edge but Cloudflare / a region block intercepted the
    /// request (protective status, or a 403 carrying an HTML/CF body).
    /// The key's validity is unknown.
    Unreachable,
}

/// Cloudflare / region-block status codes that are never the key's fault.
const EDGE_BLOCK_STATUSES: &[u16] = &[429, 503, 520, 521, 522, 523, 524, 525, 526];

/// Markers that a non-2xx body came from Cloudflare / an HTML interstitial
/// rather than the CurseForge JSON API.
const CF_BODY_MARKERS: &[&str] = &[
    "cloudflare",
    "cf-ray",
    "<!doctype html",
    "<html",
    "just a moment",
    "attention required",
    "access denied",
];

/// Classify a key-validation response. See `KeyCheckOutcome`.
pub(crate) fn classify_key_check(status: u16, body: &[u8]) -> KeyCheckOutcome {
    if (200..=299).contains(&status) {
        return KeyCheckOutcome::Ok;
    }
    if EDGE_BLOCK_STATUSES.contains(&status) {
        return KeyCheckOutcome::Unreachable;
    }
    if status == 403 {
        let hay = String::from_utf8_lossy(body).to_ascii_lowercase();
        if CF_BODY_MARKERS.iter().any(|m| hay.contains(m)) {
            return KeyCheckOutcome::Unreachable;
        }
    }
    KeyCheckOutcome::Invalid
}

#[cfg(test)]
mod key_check_tests {
    use super::{classify_key_check, KeyCheckOutcome};

    #[test]
    fn success_status_is_ok() {
        assert_eq!(classify_key_check(200, b"{}"), KeyCheckOutcome::Ok);
        assert_eq!(classify_key_check(204, b""), KeyCheckOutcome::Ok);
    }

    #[test]
    fn genuine_api_rejection_is_invalid() {
        // CurseForge returns JSON on a bad/missing key.
        let body = br#"{"error":"API key invalid"}"#;
        assert_eq!(classify_key_check(403, body), KeyCheckOutcome::Invalid);
        assert_eq!(classify_key_check(401, body), KeyCheckOutcome::Invalid);
        assert_eq!(classify_key_check(404, b"{}"), KeyCheckOutcome::Invalid);
    }

    #[test]
    fn cloudflare_html_403_is_unreachable() {
        let body = b"<!DOCTYPE html><html><head><title>Just a moment...</title>\
                     </head><body>cf-ray: 1234</body></html>";
        assert_eq!(classify_key_check(403, body), KeyCheckOutcome::Unreachable);
    }

    #[test]
    fn attention_required_403_is_unreachable() {
        let body = b"<html><body>Attention Required! | Cloudflare</body></html>";
        assert_eq!(classify_key_check(403, body), KeyCheckOutcome::Unreachable);
    }

    #[test]
    fn protective_statuses_are_unreachable() {
        for s in [429u16, 503, 520, 521, 522, 523, 524, 525, 526] {
            assert_eq!(
                classify_key_check(s, b"whatever"),
                KeyCheckOutcome::Unreachable,
                "status {s} should be Unreachable"
            );
        }
    }

    #[test]
    fn case_insensitive_body_match() {
        assert_eq!(
            classify_key_check(403, b"CLOUDFLARE blocked this request"),
            KeyCheckOutcome::Unreachable
        );
    }
}

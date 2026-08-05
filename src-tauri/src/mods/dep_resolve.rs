//! Resolve a bare loader mod-id (e.g. `balm`) to an installable platform
//! project, and verify a downloaded candidate actually provides that id.
//! The platform calls are injected so the orchestration is unit-testable.

use std::future::Future;

use crate::mods::local::{read_jar_embedded_providers, read_jar_manifest_deps, DepSide};
use crate::mods::platform::ModVersion;
use serde::Serialize;
use std::collections::HashSet;

/// Outcome of an `install_missing_into_dir` run: which dep-ids produced an
/// installed jar (by filename) vs. which could not be resolved/verified
/// automatically. Lets the UI report honestly instead of claiming silent
/// success — the unresolved ids point the user at the mod browser.
#[derive(Debug, Clone, Default, Serialize, specta::Type)]
pub struct InstallMissingReport {
    /// Filenames of jars actually downloaded + copied into the target dir.
    pub installed: Vec<String>,
    /// Dep mod-ids that could not be resolved or verified (skipped).
    pub unresolved: Vec<String>,
}

/// Case- and `_`/`-`-insensitive id normalization for cross-source matching.
fn norm_id(id: &str) -> String {
    id.trim().to_ascii_lowercase().replace('-', "_")
}

/// True iff `jar_bytes` declares `dep_id` among the mod-ids it provides — its
/// own `[[mods]]` / fabric / quilt id, a `provides` alias, or a JIJ submodule.
/// Best-effort: an unreadable jar yields `false`.
pub fn jar_provides(jar_bytes: &[u8], dep_id: &str) -> bool {
    let wanted = norm_id(dep_id);
    let Ok(manifest) = read_jar_manifest_deps(jar_bytes) else {
        return false;
    };
    manifest
        .provided
        .iter()
        .map(|p| p.mod_id.clone())
        .chain(
            read_jar_embedded_providers(jar_bytes)
                .into_iter()
                .map(|p| p.mod_id),
        )
        .any(|id| norm_id(&id) == wanted)
}

/// Normalize a loader mod-id into candidate platform slugs to try, most-likely
/// first: the id itself, then `_`<->`-` swaps. Lowercased, de-duplicated, order
/// preserved.
pub fn slug_candidates(dep_id: &str) -> Vec<String> {
    let base = dep_id.trim().to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    push(base.clone());
    push(base.replace('_', "-"));
    push(base.replace('-', "_"));
    out
}

/// Outcome of resolving a bare loader mod-id to an installable project.
// `Resolved` carries a ModVersion by value; DepResolution is a transient stack
// value returned and immediately matched, never stored in a collection, so the
// size asymmetry is irrelevant and boxing would only add indirection + ceremony.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum DepResolution {
    /// A concrete installable candidate. `needed_id` is the original loader id
    /// the candidate must `provide` (verified over the downloaded jar at install).
    /// `selection_reason` records *why* this build was picked (pin / range /
    /// newest) for honest provenance on the install plan.
    Resolved {
        candidate: ModVersion,
        needed_id: String,
        selection_reason: crate::mods::platform::SelectionReason,
    },
    /// No confident match — the surface degrades to a pre-filled search.
    Unresolved { query: String },
}

/// Resolve `dep_id` to an installable candidate, Modrinth-first then CurseForge,
/// delegating the per-platform build pick to the pure `select_dep_version`
/// kernel (pin → range → newest + reason). Network is injected as two lookup
/// closures, each returning the *full* candidate list (newest-first) for the
/// dep id — slug-exact + name-search fallback live inside the closures
/// (`modrinth_lookup`/`curseforge_lookup`):
/// - `mr_lookup(dep_id)` -> Modrinth candidates (`Err`/empty treated as a miss).
/// - `cf_lookup(dep_id)` -> CurseForge candidates (`Err`/empty treated as a miss).
///
/// `range` is the jar-manifest declared range + loader family when known (the
/// manifest-extras path threads it; the bare "Install {dep}" button passes
/// `None`). There is no author pin on this path — pins ride the platform
/// `resolve_deps` path, not bare-id resolution — so `select_dep_version` is
/// called with `pin = None`.
///
/// Modrinth is tried before CurseForge and short-circuits: CurseForge is only
/// awaited if Modrinth does not yield a pick. `Err` is dropped (treated as a
/// miss) and an empty `Ok(vec![])` is skipped, preserving the prior "error ==
/// miss" / "fall through on no hit" semantics.
pub async fn resolve_missing_dep<MF, MFut, CF, CFut>(
    dep_id: &str,
    range: Option<(&str, crate::mods::version_range::RangeFamily)>,
    mut mr_lookup: MF,
    mut cf_lookup: CF,
) -> DepResolution
where
    MF: FnMut(String) -> MFut,
    MFut: Future<Output = Result<Vec<ModVersion>, crate::error::Error>>,
    CF: FnMut(String) -> CFut,
    CFut: Future<Output = Result<Vec<ModVersion>, crate::error::Error>>,
{
    // Modrinth first — return on a pick before CurseForge is ever awaited, so a
    // Modrinth hit never triggers the CF network search.
    let mr_cands = mr_lookup(dep_id.to_string()).await;
    if let Ok(c) = mr_cands {
        if let Some((i, reason)) = crate::mods::dep_select::select_dep_version(&c, None, range) {
            return DepResolution::Resolved {
                candidate: c
                    .into_iter()
                    .nth(i)
                    .expect("index returned by select_dep_version is in range"),
                needed_id: dep_id.to_string(),
                selection_reason: reason,
            };
        }
    }
    // CurseForge fallback — only reached when Modrinth missed.
    let cf_cands = cf_lookup(dep_id.to_string()).await;
    if let Ok(c) = cf_cands {
        if let Some((i, reason)) = crate::mods::dep_select::select_dep_version(&c, None, range) {
            return DepResolution::Resolved {
                candidate: c
                    .into_iter()
                    .nth(i)
                    .expect("index returned by select_dep_version is in range"),
                needed_id: dep_id.to_string(),
                selection_reason: reason,
            };
        }
    }
    DepResolution::Unresolved {
        query: dep_id.to_string(),
    }
}

/// Modrinth candidates for a bare loader id: exact slug candidates first, then a
/// relevance search whose hits are accepted by normalized name/slug containment
/// (`name_matches`) — this is R1, recovering the modId≠slug case (e.g. loader id
/// `spore` → published slug `fungal-infection-spore`). Newest-first (Modrinth
/// `versions` is newest-first). Errors / empty pages collapse to an empty vec (a
/// miss); the downstream `jar_provides` gate is the final arbiter so a loosened
/// name match can never install a wrong jar.
pub async fn modrinth_lookup(
    mr: &crate::mods::modrinth::ModrinthClient,
    dep_id: &str,
    mc: &str,
    loader: crate::mods::platform::LoaderKind,
) -> Vec<ModVersion> {
    use crate::mods::platform::ModPlatform;
    for slug in slug_candidates(dep_id) {
        if let Ok(v) = mr.versions(&slug, Some(mc), Some(loader)).await {
            if !v.is_empty() {
                return v;
            }
        }
    }
    let q = crate::mods::platform::ModSearchQuery {
        server_only: false,
        source: crate::mods::platform::ModSource::Modrinth,
        kind: crate::mods::platform::ContentKind::Mod,
        query: dep_id.to_string(),
        mc_version: Some(mc.to_string()),
        loader: Some(loader),
        sort: crate::mods::platform::ModSort::Relevance,
        page_size: 20,
        offset: 0,
        plugin_core: None,
    };
    if let Ok(page) = mr.search(&q).await {
        for hit in page.hits {
            if crate::mods::dep_select::name_matches(dep_id, hit.slug.as_deref(), &hit.name) {
                if let Ok(v) = mr.versions(&hit.project_id, Some(mc), Some(loader)).await {
                    if !v.is_empty() {
                        return v;
                    }
                }
            }
        }
    }
    Vec::new()
}

/// CurseForge candidates. The project *identification* search is DECOUPLED from
/// loader/MC (`mc_version: None, loader: None`) because CF's `modLoaderType`
/// filter is documented as unreliable and can hide an existing project; loader
/// and MC are applied only at the subsequent `versions(...)` step. A hit is
/// accepted by exact slug OR normalized name containment (`name_matches`).
/// Errors / empty collapse to an empty vec (a miss); `jar_provides` remains the
/// final arbiter at install time.
pub async fn curseforge_lookup(
    cf: &crate::mods::curseforge::CurseForgeClient,
    dep_id: &str,
    mc: &str,
    loader: crate::mods::platform::LoaderKind,
) -> Vec<ModVersion> {
    use crate::mods::platform::ModPlatform;
    let q = crate::mods::platform::ModSearchQuery {
        server_only: false,
        source: crate::mods::platform::ModSource::Curseforge,
        kind: crate::mods::platform::ContentKind::Mod,
        query: dep_id.to_string(),
        mc_version: None, // decoupled — CF modLoaderType/gameVersion filter is unreliable
        loader: None,     // decoupled
        sort: crate::mods::platform::ModSort::Relevance,
        page_size: 20,
        offset: 0,
        plugin_core: None,
    };
    let Ok(page) = cf.search(&q).await else {
        return Vec::new();
    };
    for hit in page.hits {
        let slug_eq = hit
            .slug
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case(dep_id))
            .unwrap_or(false);
        if slug_eq || crate::mods::dep_select::name_matches(dep_id, hit.slug.as_deref(), &hit.name)
        {
            if let Ok(mut v) = cf.versions(&hit.project_id, Some(mc), Some(loader)).await {
                v.sort_by(|a, b| b.published_at.cmp(&a.published_at));
                if !v.is_empty() {
                    return v;
                }
            }
        }
    }
    Vec::new()
}

/// Resolve each loader mod-id in `mod_ids` to an installable candidate (the same
/// Modrinth-slug-first -> CurseForge path the instance "install missing required"
/// flow uses), download it through `network::` (`fetch_to_cache`), verify the jar
/// actually provides the id, and copy it into `dest`. Best-effort per id: an
/// unresolved or unverifiable id is skipped (the diagnosis recommendation still
/// points the user at the Add-ons browser), so a partial set still installs what
/// it can. `data_dir` is the shared mod-cache root; `dest` is the server's
/// `mods/` directory.
pub async fn install_missing_into_dir(
    data_dir: &std::path::Path,
    dest: &std::path::Path,
    mod_ids: &[String],
    mc_version: &str,
    loader: crate::instances::schema::LoaderKind,
    cf_key: Option<String>,
) -> crate::error::Result<InstallMissingReport> {
    use crate::mods::curseforge::CurseForgeClient;
    use crate::mods::install::{fetch_to_cache, ProgressFn};
    use crate::mods::modrinth::ModrinthClient;

    let mr = ModrinthClient::new();
    let cf = CurseForgeClient::with_base_and_key("https://api.curseforge.com", cf_key);
    let nop: ProgressFn = Box::new(|_, _, _| {});
    let mut report = InstallMissingReport::default();

    tokio::fs::create_dir_all(dest)
        .await
        .map_err(|e| crate::error::Error::io(dest.display().to_string(), e))?;

    for dep_id in mod_ids {
        // No manifest range context on the bare-id install path → `None`.
        let resolution = resolve_missing_dep(
            dep_id,
            None,
            |id| {
                let mr = &mr;
                async move { Ok(modrinth_lookup(mr, &id, mc_version, loader).await) }
            },
            |id| {
                let cf = &cf;
                async move { Ok(curseforge_lookup(cf, &id, mc_version, loader).await) }
            },
        )
        .await;

        let DepResolution::Resolved {
            candidate,
            needed_id,
            selection_reason,
        } = resolution
        else {
            report.unresolved.push(dep_id.clone());
            continue;
        };
        crate::diag!(
            "dep_resolve: {dep_id} -> {} ({selection_reason:?})",
            candidate.version_id
        );
        let Some(sha) = candidate
            .primary_file
            .sha1
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase())
        else {
            report.unresolved.push(dep_id.clone());
            continue;
        };
        let cached = fetch_to_cache(
            data_dir,
            &candidate.primary_file.url,
            &sha,
            candidate.primary_file.size,
            "servers",
            &nop,
        )
        .await?;
        let bytes = tokio::fs::read(&cached.path)
            .await
            .map_err(|e| crate::error::Error::io("<dep-candidate-cache>", e))?;
        if !jar_provides(&bytes, &needed_id) {
            report.unresolved.push(dep_id.clone());
            continue;
        }
        // Guard the platform-supplied filename before joining into dest.
        if !crate::mods::modpack::path_safety::is_safe_filename(&candidate.primary_file.filename) {
            report.unresolved.push(dep_id.clone());
            continue;
        }
        let out = dest.join(&candidate.primary_file.filename);
        tokio::fs::copy(&cached.path, &out)
            .await
            .map_err(|e| crate::error::Error::io(out.display().to_string(), e))?;
        report
            .installed
            .push(candidate.primary_file.filename.clone());
    }
    Ok(report)
}

/// A manifest-discovered required dependency the platform metadata omitted,
/// resolved to an installable candidate. `needed_id` is verified against the
/// downloaded jar at install time. `selection_reason` carries why this build was
/// chosen (range-aware) so the install plan can surface honest provenance.
#[derive(Debug, Clone)]
pub struct ExtraRoot {
    pub needed_id: String,
    pub candidate: ModVersion,
    pub selection_reason: crate::mods::platform::SelectionReason,
}

/// Read a primary jar's manifest and resolve every *required* dependency it
/// declares (dropping server-only and loader/MC ids — the latter already
/// excluded by `read_jar_manifest_deps`). `resolve` is called once per distinct
/// dep with the full `DeclaredDep` (so the caller can thread its range + family
/// into selection). Returns resolved candidates and the dep-ids that could not
/// be resolved. Best-effort: an unreadable jar yields empty vecs.
pub async fn manifest_extra_roots<F, Fut>(
    primary_bytes: &[u8],
    mut resolve: F,
) -> (Vec<ExtraRoot>, Vec<String>)
where
    F: FnMut(crate::mods::local::DeclaredDep) -> Fut,
    Fut: Future<Output = DepResolution>,
{
    let Ok(manifest) = read_jar_manifest_deps(primary_bytes) else {
        return (Vec::new(), Vec::new());
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    // No instance here: this runs while installing a jar, and a multi-loader
    // jar must not drag in the other loader's requirements.
    let deps: Vec<_> = manifest.deps_without_instance().cloned().collect();
    for dep in deps {
        // Only a genuine requirement is auto-installed. Before dependency kinds
        // were modelled, an `incompatible` declaration parsed as required here,
        // so installing such a jar pulled in the very mod it declares itself
        // incompatible with.
        if !dep.kind.is_required() || dep.side == DepSide::Server {
            continue;
        }
        if !seen.insert(norm_id(&dep.dep_id)) {
            continue;
        }
        match resolve(dep.clone()).await {
            DepResolution::Resolved {
                candidate,
                needed_id,
                selection_reason,
            } => resolved.push(ExtraRoot {
                needed_id,
                candidate,
                selection_reason,
            }),
            DepResolution::Unresolved { query } => unresolved.push(query),
        }
    }
    (resolved, unresolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    fn jar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut buf));
            for (name, body) in entries {
                w.start_file(*name, SimpleFileOptions::default()).unwrap();
                w.write_all(body).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn slug_candidates_covers_underscore_and_hyphen_forms() {
        assert_eq!(slug_candidates("balm"), vec!["balm"]);
        assert_eq!(
            slug_candidates("forge_config_api_port"),
            vec!["forge_config_api_port", "forge-config-api-port"]
        );
        assert_eq!(
            slug_candidates("Cloth-Config"),
            vec!["cloth-config", "cloth_config"]
        );
        assert!(slug_candidates("   ").is_empty());
    }

    #[test]
    fn jar_provides_matches_own_id_case_and_separator_insensitive() {
        let balm = jar(&[(
            "META-INF/neoforge.mods.toml",
            b"[[mods]]\nmodId=\"balm\"\nversion=\"9.0.0\"\n",
        )]);
        assert!(jar_provides(&balm, "balm"));
        assert!(jar_provides(&balm, "BALM"));
        assert!(!jar_provides(&balm, "waystones"));
    }

    #[test]
    fn jar_provides_matches_jij_submodule() {
        let inner = jar(&[(
            "fabric.mod.json",
            br#"{"id":"cloth-config","version":"1.0"}"#,
        )]);
        let outer = jar(&[
            ("fabric.mod.json", br#"{"id":"somelib","version":"1.0"}"#),
            ("META-INF/jars/cloth-config.jar", &inner),
        ]);
        assert!(jar_provides(&outer, "cloth_config"));
    }

    #[test]
    fn jar_provides_false_on_unreadable_jar() {
        assert!(!jar_provides(b"not a zip", "anything"));
    }

    use crate::mods::platform::{LoaderKind, ModFile, ModSource, ModVersion};
    use std::future::ready;

    fn mv(source: ModSource, project_id: &str) -> ModVersion {
        ModVersion {
            source,
            project_id: project_id.into(),
            version_id: format!("{project_id}-v"),
            name: project_id.into(),
            version_number: "1.0".into(),
            mc_versions: vec!["1.20.4".into()],
            loaders: vec![LoaderKind::NeoForge],
            primary_file: ModFile {
                filename: format!("{project_id}.jar"),
                url: format!("https://cdn/{project_id}.jar"),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: vec![],
            published_at: None,
        }
    }

    /// Like `mv` but with an explicit `version_number` (and a version_id derived
    /// from it, so distinct builds of one project stay distinguishable).
    fn mvn(source: ModSource, project_id: &str, version_number: &str) -> ModVersion {
        ModVersion {
            version_id: format!("{project_id}-{version_number}"),
            version_number: version_number.into(),
            ..mv(source, project_id)
        }
    }

    use crate::mods::platform::SelectionReason;

    #[tokio::test]
    async fn resolve_prefers_modrinth_then_cf() {
        let r = resolve_missing_dep(
            "balm",
            None,
            |_id| ready(Ok(vec![mv(ModSource::Modrinth, "balm")])),
            |_id| ready(Ok(vec![mv(ModSource::Curseforge, "531761")])),
        )
        .await;
        match r {
            DepResolution::Resolved {
                candidate,
                needed_id,
                selection_reason,
            } => {
                assert_eq!(candidate.source, ModSource::Modrinth);
                assert_eq!(candidate.project_id, "balm");
                assert_eq!(needed_id, "balm");
                assert_eq!(selection_reason, SelectionReason::NewestNoPin);
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_falls_back_to_cf_when_modrinth_empty() {
        let r = resolve_missing_dep(
            "balm",
            None,
            |_id| ready(Ok(vec![])),
            |_id| ready(Ok(vec![mv(ModSource::Curseforge, "531761")])),
        )
        .await;
        assert!(matches!(
            r,
            DepResolution::Resolved { candidate, .. } if candidate.source == ModSource::Curseforge
        ));
    }

    #[tokio::test]
    async fn resolve_does_not_call_cf_when_modrinth_resolves() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let cf_called = AtomicBool::new(false);
        let r = resolve_missing_dep(
            "balm",
            None,
            |_id| ready(Ok(vec![mv(ModSource::Modrinth, "balm")])),
            |_id| {
                cf_called.store(true, Ordering::SeqCst);
                ready(Ok(vec![]))
            },
        )
        .await;
        assert!(matches!(r, DepResolution::Resolved { .. }));
        assert!(
            !cf_called.load(Ordering::SeqCst),
            "CF must not be called when Modrinth resolves"
        );
    }

    #[tokio::test]
    async fn resolve_unresolved_when_both_empty() {
        let r = resolve_missing_dep(
            "totally-unknown-lib",
            None,
            |_id| ready(Ok(vec![])),
            |_id| ready(Ok(vec![])),
        )
        .await;
        assert!(matches!(r, DepResolution::Unresolved { query } if query == "totally-unknown-lib"));
    }

    #[tokio::test]
    async fn resolve_error_is_a_miss() {
        // Modrinth errors → dropped; CF empty → overall miss.
        let r = resolve_missing_dep(
            "balm",
            None,
            |_id| ready(Err(crate::error::Error::ModsSha1Unavailable)),
            |_id| ready(Ok(vec![])),
        )
        .await;
        assert!(matches!(r, DepResolution::Unresolved { .. }));
    }

    #[tokio::test]
    async fn resolve_threads_range_into_selection() {
        // Two compatible builds, newest-first. A bounded range excludes the
        // newest → the older satisfying build is chosen, RangeConstrained.
        let r = resolve_missing_dep(
            "azurelib",
            Some((
                "[2.0.39,2.1)",
                crate::mods::version_range::RangeFamily::Maven,
            )),
            |_id| {
                ready(Ok(vec![
                    mvn(ModSource::Modrinth, "azurelib", "3.1.11"),
                    mvn(ModSource::Modrinth, "azurelib", "2.0.41"),
                ]))
            },
            |_id| ready(Ok(vec![])),
        )
        .await;
        match r {
            DepResolution::Resolved {
                candidate,
                selection_reason,
                ..
            } => {
                assert_eq!(candidate.version_number, "2.0.41");
                assert_eq!(selection_reason, SelectionReason::RangeConstrained);
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn manifest_extra_roots_resolves_only_unforgotten_required_deps() {
        let waystones = jar(&[(
            "META-INF/neoforge.mods.toml",
            b"[[mods]]\nmodId=\"waystones\"\n\
              [[dependencies.waystones]]\nmodId=\"balm\"\ntype=\"required\"\nversionRange=\"[9.0.0,)\"\n\
              [[dependencies.waystones]]\nmodId=\"jei\"\ntype=\"optional\"\n\
              [[dependencies.waystones]]\nmodId=\"srv\"\ntype=\"required\"\nside=\"SERVER\"\n",
        )]);

        let (resolved, unresolved) = manifest_extra_roots(&waystones, |dep| {
            let dep_id = dep.dep_id.clone();
            async move {
                if dep_id == "balm" {
                    DepResolution::Resolved {
                        candidate: mv(crate::mods::platform::ModSource::Modrinth, "balm"),
                        needed_id: "balm".into(),
                        selection_reason: SelectionReason::NewestNoPin,
                    }
                } else {
                    DepResolution::Unresolved { query: dep_id }
                }
            }
        })
        .await;

        assert_eq!(resolved.len(), 1, "{resolved:?}");
        assert_eq!(resolved[0].needed_id, "balm");
        assert_eq!(resolved[0].candidate.project_id, "balm");
        assert_eq!(resolved[0].selection_reason, SelectionReason::NewestNoPin);
        assert!(unresolved.is_empty(), "{unresolved:?}");
    }

    #[tokio::test]
    async fn manifest_extra_roots_passes_declared_dep_to_resolver() {
        // The resolve closure receives the full DeclaredDep (id + range), not a
        // bare String — proves the range is available to thread into selection.
        let waystones = jar(&[(
            "META-INF/neoforge.mods.toml",
            b"[[mods]]\nmodId=\"waystones\"\n\
              [[dependencies.waystones]]\nmodId=\"balm\"\ntype=\"required\"\nversionRange=\"[9.0.0,)\"\n",
        )]);
        let (resolved, _unresolved) =
            manifest_extra_roots(&waystones, |dep: crate::mods::local::DeclaredDep| {
                // Assert the full DeclaredDep (id + range) made it through.
                assert_eq!(dep.dep_id, "balm");
                assert_eq!(dep.range, "[9.0.0,)");
                async move {
                    DepResolution::Resolved {
                        candidate: mv(crate::mods::platform::ModSource::Modrinth, "balm"),
                        needed_id: "balm".into(),
                        selection_reason: SelectionReason::NewestNoPin,
                    }
                }
            })
            .await;
        assert_eq!(resolved.len(), 1);
    }

    #[tokio::test]
    async fn manifest_extra_roots_collects_unresolved_ids() {
        let m = jar(&[(
            "fabric.mod.json",
            br#"{"id":"mymod","depends":{"weirdlib":"*"}}"#,
        )]);
        let (resolved, unresolved) = manifest_extra_roots(&m, |dep| async move {
            DepResolution::Unresolved { query: dep.dep_id }
        })
        .await;
        assert!(resolved.is_empty());
        assert_eq!(unresolved, vec!["weirdlib"]);
    }

    #[tokio::test]
    async fn manifest_extra_roots_empty_for_descriptorless_jar() {
        let m = jar(&[("foo.txt", b"x")]);
        let (resolved, unresolved) = manifest_extra_roots(&m, |dep| async move {
            DepResolution::Unresolved { query: dep.dep_id }
        })
        .await;
        assert!(resolved.is_empty() && unresolved.is_empty());
    }
}

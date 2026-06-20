//! Resolve a bare loader mod-id (e.g. `balm`) to an installable platform
//! project, and verify a downloaded candidate actually provides that id.
//! The platform calls are injected so the orchestration is unit-testable.

use std::future::Future;

use crate::mods::local::{read_jar_embedded_providers, read_jar_manifest_deps, DepSide};
use crate::mods::platform::ModVersion;
use std::collections::HashSet;

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
#[derive(Debug, Clone)]
pub enum DepResolution {
    /// A concrete installable candidate. `needed_id` is the original loader id
    /// the candidate must `provide` (verified over the downloaded jar at install).
    Resolved {
        candidate: ModVersion,
        needed_id: String,
    },
    /// No confident match — the surface degrades to a pre-filled search.
    Unresolved { query: String },
}

/// Resolve `dep_id` to an installable candidate, Modrinth-slug-first then
/// CurseForge. Network is injected:
/// - `mr_versions(slug)` -> Modrinth versions for that slug, already filtered to
///   the target mc+loader, newest-first (`Err` is treated as a miss).
/// - `cf_lookup(dep_id)` -> best CurseForge candidate (search -> exact-slug match
///   -> newest version), `None` when none (`Err` is treated as a miss).
pub async fn resolve_missing_dep<MF, MFut, CF, CFut>(
    dep_id: &str,
    mut mr_versions: MF,
    mut cf_lookup: CF,
) -> DepResolution
where
    MF: FnMut(String) -> MFut,
    MFut: Future<Output = Result<Vec<ModVersion>, crate::error::Error>>,
    CF: FnMut(String) -> CFut,
    CFut: Future<Output = Result<Option<ModVersion>, crate::error::Error>>,
{
    for slug in slug_candidates(dep_id) {
        if let Ok(versions) = mr_versions(slug).await {
            if let Some(candidate) = versions.into_iter().next() {
                return DepResolution::Resolved {
                    candidate,
                    needed_id: dep_id.to_string(),
                };
            }
        }
    }
    if let Ok(Some(candidate)) = cf_lookup(dep_id.to_string()).await {
        return DepResolution::Resolved {
            candidate,
            needed_id: dep_id.to_string(),
        };
    }
    DepResolution::Unresolved {
        query: dep_id.to_string(),
    }
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
) -> crate::error::Result<()> {
    use crate::mods::curseforge::CurseForgeClient;
    use crate::mods::install::{fetch_to_cache, ProgressFn};
    use crate::mods::modrinth::ModrinthClient;
    use crate::mods::platform::{ContentKind, ModPlatform, ModSearchQuery, ModSort, ModSource};

    let mr = ModrinthClient::new();
    let cf = CurseForgeClient::with_base_and_key("https://api.curseforge.com", cf_key);
    let nop: ProgressFn = Box::new(|_, _, _| {});

    tokio::fs::create_dir_all(dest)
        .await
        .map_err(|e| crate::error::Error::io(dest.display().to_string(), e))?;

    for dep_id in mod_ids {
        let resolution = resolve_missing_dep(
            dep_id,
            |slug| {
                let mr = &mr;
                async move { mr.versions(&slug, Some(mc_version), Some(loader)).await }
            },
            |id| {
                let cf = &cf;
                async move {
                    let page = cf
                        .search(&ModSearchQuery {
                            source: ModSource::Curseforge,
                            kind: ContentKind::Mod,
                            query: id.clone(),
                            mc_version: Some(mc_version.to_string()),
                            loader: Some(loader),
                            sort: ModSort::Relevance,
                            page_size: 20,
                            offset: 0,
                        })
                        .await?;
                    let Some(hit) = page.hits.into_iter().find(|h| {
                        h.slug
                            .as_deref()
                            .map(|s| s.eq_ignore_ascii_case(&id))
                            .unwrap_or(false)
                    }) else {
                        return Ok(None);
                    };
                    let mut versions = cf
                        .versions(&hit.project_id, Some(mc_version), Some(loader))
                        .await?;
                    versions.sort_by(|a, b| b.published_at.cmp(&a.published_at));
                    Ok(versions.into_iter().next())
                }
            },
        )
        .await;

        let DepResolution::Resolved {
            candidate,
            needed_id,
        } = resolution
        else {
            continue;
        };
        let Some(sha) = candidate
            .primary_file
            .sha1
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase())
        else {
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
        let bytes = tokio::fs::read(&cached)
            .await
            .map_err(|e| crate::error::Error::io("<dep-candidate-cache>", e))?;
        if !jar_provides(&bytes, &needed_id) {
            continue;
        }
        // Guard the platform-supplied filename before joining into dest.
        if !crate::mods::modpack::path_safety::is_safe_filename(&candidate.primary_file.filename) {
            continue;
        }
        let out = dest.join(&candidate.primary_file.filename);
        tokio::fs::copy(&cached, &out)
            .await
            .map_err(|e| crate::error::Error::io(out.display().to_string(), e))?;
    }
    Ok(())
}

/// A manifest-discovered required dependency the platform metadata omitted,
/// resolved to an installable candidate. `needed_id` is verified against the
/// downloaded jar at install time.
#[derive(Debug, Clone)]
pub struct ExtraRoot {
    pub needed_id: String,
    pub candidate: ModVersion,
}

/// Read a primary jar's manifest and resolve every *required* dependency it
/// declares (dropping server-only and loader/MC ids — the latter already
/// excluded by `read_jar_manifest_deps`). `resolve` is called once per distinct
/// dep-id. Returns resolved candidates and the dep-ids that could not be
/// resolved. Best-effort: an unreadable jar yields empty vecs.
pub async fn manifest_extra_roots<F, Fut>(
    primary_bytes: &[u8],
    mut resolve: F,
) -> (Vec<ExtraRoot>, Vec<String>)
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = DepResolution>,
{
    let Ok(manifest) = read_jar_manifest_deps(primary_bytes) else {
        return (Vec::new(), Vec::new());
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for dep in manifest.deps {
        if !dep.required || dep.side == DepSide::Server {
            continue;
        }
        if !seen.insert(norm_id(&dep.dep_id)) {
            continue;
        }
        match resolve(dep.dep_id.clone()).await {
            DepResolution::Resolved {
                candidate,
                needed_id,
            } => resolved.push(ExtraRoot {
                needed_id,
                candidate,
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
            },
            deps: vec![],
            published_at: None,
        }
    }

    #[tokio::test]
    async fn resolve_prefers_modrinth_slug_match() {
        let r = resolve_missing_dep(
            "balm",
            |slug| {
                ready(Ok(if slug == "balm" {
                    vec![mv(ModSource::Modrinth, "balm")]
                } else {
                    vec![]
                }))
            },
            |_id| ready(Ok(None)),
        )
        .await;
        match r {
            DepResolution::Resolved {
                candidate,
                needed_id,
            } => {
                assert_eq!(candidate.source, ModSource::Modrinth);
                assert_eq!(candidate.project_id, "balm");
                assert_eq!(needed_id, "balm");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_falls_back_to_curseforge_when_modrinth_empty() {
        let r = resolve_missing_dep(
            "balm",
            |_slug| ready(Ok(vec![])),
            |_id| ready(Ok(Some(mv(ModSource::Curseforge, "531761")))),
        )
        .await;
        assert!(matches!(
            r,
            DepResolution::Resolved { candidate, .. } if candidate.source == ModSource::Curseforge
        ));
    }

    #[tokio::test]
    async fn resolve_unresolved_when_neither_platform_finds_it() {
        let r = resolve_missing_dep(
            "totally-unknown-lib",
            |_slug| ready(Ok(vec![])),
            |_id| ready(Ok(None)),
        )
        .await;
        assert!(matches!(r, DepResolution::Unresolved { query } if query == "totally-unknown-lib"));
    }

    #[tokio::test]
    async fn resolve_treats_platform_error_as_a_miss() {
        let r = resolve_missing_dep(
            "balm",
            |_slug| ready(Err(crate::error::Error::ModsSha1Unavailable)),
            |_id| ready(Ok(None)),
        )
        .await;
        assert!(matches!(r, DepResolution::Unresolved { .. }));
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

        let (resolved, unresolved) = manifest_extra_roots(&waystones, |id| {
            let id2 = id.clone();
            async move {
                if id2 == "balm" {
                    DepResolution::Resolved {
                        candidate: mv(crate::mods::platform::ModSource::Modrinth, "balm"),
                        needed_id: "balm".into(),
                    }
                } else {
                    DepResolution::Unresolved { query: id2 }
                }
            }
        })
        .await;

        assert_eq!(resolved.len(), 1, "{resolved:?}");
        assert_eq!(resolved[0].needed_id, "balm");
        assert_eq!(resolved[0].candidate.project_id, "balm");
        assert!(unresolved.is_empty(), "{unresolved:?}");
    }

    #[tokio::test]
    async fn manifest_extra_roots_collects_unresolved_ids() {
        let m = jar(&[(
            "fabric.mod.json",
            br#"{"id":"mymod","depends":{"weirdlib":"*"}}"#,
        )]);
        let (resolved, unresolved) =
            manifest_extra_roots(
                &m,
                |id| async move { DepResolution::Unresolved { query: id } },
            )
            .await;
        assert!(resolved.is_empty());
        assert_eq!(unresolved, vec!["weirdlib"]);
    }

    #[tokio::test]
    async fn manifest_extra_roots_empty_for_descriptorless_jar() {
        let m = jar(&[("foo.txt", b"x")]);
        let (resolved, unresolved) =
            manifest_extra_roots(
                &m,
                |id| async move { DepResolution::Unresolved { query: id } },
            )
            .await;
        assert!(resolved.is_empty() && unresolved.is_empty());
    }
}

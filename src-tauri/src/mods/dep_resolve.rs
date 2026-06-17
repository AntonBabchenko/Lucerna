//! Resolve a bare loader mod-id (e.g. `balm`) to an installable platform
//! project, and verify a downloaded candidate actually provides that id.
//! The platform calls are injected so the orchestration is unit-testable.

use std::future::Future;

use crate::mods::local::{read_jar_embedded_providers, read_jar_manifest_deps};
use crate::mods::platform::ModVersion;

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
}

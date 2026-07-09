//! Pure transitive dependency-closure resolver. Decoupled from the network:
//! callers inject an async `fetch` that, given a project key, returns that
//! project's resolved direct dependencies already classified. The walk
//! follows ONLY required edges transitively; optionals are surfaced one level
//! at a time (the caller decides whether to opt in, then re-walks from the
//! chosen optional). Cycle-safe, deduplicating, and prunes already-installed
//! projects and loader projects.

use std::collections::{HashMap, HashSet};

use crate::mods::platform::{DepProjectRef, ModSource, ModVersion, PlannedDep, SelectionReason};

/// Stable identity for dedup/cycle/skip checks. Modrinth keys on the
/// project_id string; CurseForge on the numeric mod_id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProjectKey {
    Modrinth(String),
    Curseforge(u32),
}

impl ProjectKey {
    pub fn of_version(v: &ModVersion) -> ProjectKey {
        match v.source {
            ModSource::Modrinth => ProjectKey::Modrinth(v.project_id.clone()),
            // CF project_id is always a numeric mod id; unwrap_or(0) is a defensive fallback (never expected in practice).
            ModSource::Curseforge => ProjectKey::Curseforge(v.project_id.parse().unwrap_or(0)),
            // TODO(ftb): placeholder — FTB versions are dead in this path today (no FTB mod browser /
            // dep resolution). If a future task makes FTB mods enter dedup/dep-graph keying, introduce
            // ProjectKey::Ftb instead of borrowing the Modrinth tag, to avoid a numeric-id collision
            // with real Modrinth ids.
            ModSource::Ftb => ProjectKey::Modrinth(v.project_id.clone()),
            // TODO(atlauncher): placeholder — ATLauncher versions are dead in this path today.
            ModSource::Atlauncher => ProjectKey::Modrinth(v.project_id.clone()),
            // TODO(hangar): placeholder — the Hangar client ships no dependency links (its
            // versions carry empty `deps` and resolve_deps returns an empty plan), so
            // Hangar versions stay dead in this path. Borrows the Modrinth tag like the
            // FTB/ATLauncher stubs above; introduce ProjectKey::Hangar before Hangar
            // plugins can enter dedup/dep-graph keying, to avoid a collision with real
            // Modrinth ids.
            ModSource::Hangar => ProjectKey::Modrinth(v.project_id.clone()),
        }
    }
    pub fn of_ref(r: &DepProjectRef) -> ProjectKey {
        match r {
            DepProjectRef::Modrinth { project_id, .. } => ProjectKey::Modrinth(project_id.clone()),
            DepProjectRef::Curseforge { mod_id, .. } => ProjectKey::Curseforge(*mod_id),
        }
    }
}

/// One node's resolved direct dependencies, already classified by the fetcher.
#[derive(Debug, Clone, Default)]
pub struct FetchedDeps {
    pub required: Vec<ResolvedNode>,
    pub optional: Vec<ResolvedNode>,
    pub incompatible: Vec<DepProjectRef>,
    pub unresolvable: Vec<DepProjectRef>,
}

#[derive(Debug, Clone)]
pub struct ResolvedNode {
    pub version: ModVersion,
    pub is_loader: bool,
    pub selection_reason: SelectionReason,
}

/// The transitive closure of REQUIRED dependencies of `roots`.
#[derive(Debug, Clone, Default)]
pub struct Closure {
    pub required: Vec<PlannedDep>,
    pub loader_keys: Vec<ProjectKey>,
    pub incompatible: Vec<DepProjectRef>,
    pub unresolvable: Vec<DepProjectRef>,
}

/// Walk the required-dependency graph from `roots`, returning the
/// deduplicated transitive required closure. `installed` and the roots' own
/// keys are pruned. `fetch(version)` returns that version's classified direct
/// deps; memoized here by `ProjectKey` so each project is fetched at most once.
/// Cycle-safe via the `visited` set.
///
/// `installed_filenames` (lowercased jar filenames of mods already installed
/// and enabled in the instance) prunes dependencies by filename in addition to
/// `ProjectKey`. This is how a dependency is recognised as satisfied across
/// sources: the same logical mod (e.g. Balm) installed from Modrinth and
/// referenced as a CurseForge dependency carries the same jar filename even
/// though its source-specific project ids differ. Skipping by filename here
/// also avoids walking the satisfied dependency's own subtree.
pub async fn resolve_closure<F, Fut>(
    roots: &[ModVersion],
    installed: &HashSet<ProjectKey>,
    installed_filenames: &HashSet<String>,
    mut fetch: F,
) -> Result<Closure, crate::error::Error>
where
    F: FnMut(ModVersion) -> Fut,
    Fut: std::future::Future<Output = Result<FetchedDeps, crate::error::Error>>,
{
    let mut closure = Closure::default();
    let mut visited: HashSet<ProjectKey> = HashSet::new();
    let mut loader_seen: HashSet<ProjectKey> = HashSet::new();
    let mut cache: HashMap<ProjectKey, FetchedDeps> = HashMap::new();

    let mut frontier: Vec<PlannedDep> = Vec::new();
    for r in roots {
        visited.insert(ProjectKey::of_version(r));
    }
    for r in roots {
        let deps = fetch_memo(&mut cache, &mut fetch, r.clone()).await?;
        for n in deps.required.iter().chain(deps.optional.iter()) {
            if n.is_loader {
                let lk = ProjectKey::of_version(&n.version);
                if loader_seen.insert(lk.clone()) {
                    closure.loader_keys.push(lk);
                }
            }
        }
        enqueue(&deps, &mut frontier, &mut closure);
    }

    while let Some(p) = frontier.pop() {
        let key = ProjectKey::of_version(&p.version);
        if visited.contains(&key)
            || installed.contains(&key)
            || installed_filenames.contains(&p.version.primary_file.filename.to_ascii_lowercase())
        {
            continue;
        }
        visited.insert(key.clone());
        // `fetch_memo` consumes the version, so clone it before moving `p` into
        // the closure's required list (which keeps the reason alongside it).
        let deps = fetch_memo(&mut cache, &mut fetch, p.version.clone()).await?;
        closure.required.push(p);
        for n in deps.required.iter().chain(deps.optional.iter()) {
            if n.is_loader {
                let lk = ProjectKey::of_version(&n.version);
                if loader_seen.insert(lk.clone()) {
                    closure.loader_keys.push(lk);
                }
            }
        }
        enqueue(&deps, &mut frontier, &mut closure);
    }
    Ok(closure)
}

/// Push a node's required NON-loader deps onto the frontier; merge
/// incompatible/unresolvable into the closure (deduped).
fn enqueue(deps: &FetchedDeps, frontier: &mut Vec<PlannedDep>, closure: &mut Closure) {
    for n in &deps.required {
        if n.is_loader {
            continue; // loaders are recorded by the caller loop, never installed/recursed
        }
        frontier.push(PlannedDep {
            version: n.version.clone(),
            selection_reason: n.selection_reason,
        });
    }
    for r in &deps.incompatible {
        if !closure.incompatible.iter().any(|x| same_ref(x, r)) {
            closure.incompatible.push(r.clone());
        }
    }
    for r in &deps.unresolvable {
        if !closure.unresolvable.iter().any(|x| same_ref(x, r)) {
            closure.unresolvable.push(r.clone());
        }
    }
}

fn same_ref(a: &DepProjectRef, b: &DepProjectRef) -> bool {
    ProjectKey::of_ref(a) == ProjectKey::of_ref(b)
}

async fn fetch_memo<F, Fut>(
    cache: &mut HashMap<ProjectKey, FetchedDeps>,
    fetch: &mut F,
    v: ModVersion,
) -> Result<FetchedDeps, crate::error::Error>
where
    F: FnMut(ModVersion) -> Fut,
    Fut: std::future::Future<Output = Result<FetchedDeps, crate::error::Error>>,
{
    let key = ProjectKey::of_version(&v);
    if let Some(hit) = cache.get(&key) {
        return Ok(hit.clone());
    }
    let deps = fetch(v).await?;
    cache.insert(key, deps.clone());
    Ok(deps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::platform::{
        DepKind, DepProjectRef, ModDepLink, ModFile, ModSource, ModVersion,
    };
    use std::collections::HashMap as Map;

    fn mv(id: &str, deps: Vec<(&str, DepKind)>) -> ModVersion {
        ModVersion {
            source: ModSource::Modrinth,
            project_id: id.into(),
            version_id: format!("{id}-v"),
            name: id.into(),
            version_number: "1.0".into(),
            mc_versions: vec!["1.21.1".into()],
            loaders: vec![],
            primary_file: ModFile {
                filename: format!("{id}.jar"),
                url: format!("https://cdn.modrinth.com/{id}.jar"),
                sha1: Some("aa".into()),
                size: 1.0,
                distribution_allowed: true,
                sha256: None,
            },
            deps: deps
                .into_iter()
                .map(|(d, k)| ModDepLink {
                    kind: k,
                    project_ref: DepProjectRef::Modrinth {
                        project_id: d.into(),
                        version_id: None,
                    },
                })
                .collect(),
            published_at: None,
        }
    }

    fn fetcher(
        graph: Map<String, (Vec<String>, Vec<String>, Vec<String>)>,
    ) -> impl FnMut(ModVersion) -> std::future::Ready<Result<FetchedDeps, crate::error::Error>>
    {
        move |v: ModVersion| {
            let (req, opt, loaders) = graph.get(&v.project_id).cloned().unwrap_or_default();
            let node = |id: &String, is_loader: bool| ResolvedNode {
                version: mv(id, vec![]),
                is_loader,
                selection_reason: SelectionReason::NewestNoPin,
            };
            let mut required: Vec<ResolvedNode> =
                req.iter().map(|r| node(r, loaders.contains(r))).collect();
            for l in &loaders {
                if !req.contains(l) {
                    required.push(node(l, true));
                }
            }
            let optional = opt.iter().map(|o| node(o, false)).collect();
            std::future::ready(Ok(FetchedDeps {
                required,
                optional,
                incompatible: vec![],
                unresolvable: vec![],
            }))
        }
    }

    fn g(
        pairs: &[(&str, &[&str], &[&str], &[&str])],
    ) -> Map<String, (Vec<String>, Vec<String>, Vec<String>)> {
        pairs
            .iter()
            .map(|(id, req, opt, loaders)| {
                (
                    id.to_string(),
                    (
                        req.iter().map(|s| s.to_string()).collect(),
                        opt.iter().map(|s| s.to_string()).collect(),
                        loaders.iter().map(|s| s.to_string()).collect(),
                    ),
                )
            })
            .collect()
    }

    fn keys(c: &Closure) -> Vec<String> {
        c.required
            .iter()
            .map(|p| p.version.project_id.clone())
            .collect()
    }

    #[tokio::test]
    async fn xaeros_world_map_pulls_forge_config_api_port_via_optional_open_parties() {
        let graph = g(&[
            ("open_parties", &["forge_config_api_port"], &[], &[]),
            ("forge_config_api_port", &[], &[], &[]),
        ]);
        let roots = vec![mv(
            "open_parties",
            vec![("forge_config_api_port", DepKind::Required)],
        )];
        let c = resolve_closure(&roots, &HashSet::new(), &HashSet::new(), fetcher(graph))
            .await
            .unwrap();
        assert!(keys(&c).contains(&"forge_config_api_port".to_string()));
    }

    #[tokio::test]
    async fn collects_transitive_required_chain() {
        let graph = g(&[
            ("a", &["b"], &[], &[]),
            ("b", &["c"], &[], &[]),
            ("c", &[], &[], &[]),
        ]);
        let roots = vec![mv("a", vec![("b", DepKind::Required)])];
        let c = resolve_closure(&roots, &HashSet::new(), &HashSet::new(), fetcher(graph))
            .await
            .unwrap();
        let mut k = keys(&c);
        k.sort();
        assert_eq!(k, vec!["b".to_string(), "c".to_string()]);
    }

    #[tokio::test]
    async fn closure_carries_selection_reason() {
        // The fetcher emits a NON-default reason (PinHonored) for dep "b"; the
        // assertion is on that exact reason, so the test would fail if the walk
        // reset the reason to the NewestNoPin default. (The shared `fetcher`
        // helper hard-codes NewestNoPin, which would make this vacuous — use an
        // inline fetcher instead, mirroring `unresolvable_dep_bubbles_up_from_depth`.)
        use std::future::ready;
        let fetch = move |v: ModVersion| {
            let deps = match v.project_id.as_str() {
                "a" => FetchedDeps {
                    required: vec![ResolvedNode {
                        version: mv("b", vec![]),
                        is_loader: false,
                        selection_reason: SelectionReason::PinHonored,
                    }],
                    ..Default::default()
                },
                _ => FetchedDeps::default(),
            };
            ready(Ok::<_, crate::error::Error>(deps))
        };
        let roots = vec![mv("a", vec![("b", DepKind::Required)])];
        let c = resolve_closure(&roots, &HashSet::new(), &HashSet::new(), fetch)
            .await
            .unwrap();
        assert_eq!(c.required.len(), 1);
        assert_eq!(c.required[0].selection_reason, SelectionReason::PinHonored);
    }

    #[tokio::test]
    async fn cycle_terminates() {
        let graph = g(&[("a", &["b"], &[], &[]), ("b", &["a"], &[], &[])]);
        let roots = vec![mv("a", vec![("b", DepKind::Required)])];
        let c = resolve_closure(&roots, &HashSet::new(), &HashSet::new(), fetcher(graph))
            .await
            .unwrap();
        assert_eq!(keys(&c), vec!["b".to_string()]);
    }

    #[tokio::test]
    async fn dedups_diamond() {
        let graph = g(&[
            ("a", &["b", "c"], &[], &[]),
            ("b", &["d"], &[], &[]),
            ("c", &["d"], &[], &[]),
            ("d", &[], &[], &[]),
        ]);
        let roots = vec![mv(
            "a",
            vec![("b", DepKind::Required), ("c", DepKind::Required)],
        )];
        let c = resolve_closure(&roots, &HashSet::new(), &HashSet::new(), fetcher(graph))
            .await
            .unwrap();
        assert_eq!(
            c.required
                .iter()
                .filter(|p| p.version.project_id == "d")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn skips_already_installed() {
        let graph = g(&[("a", &["b"], &[], &[]), ("b", &[], &[], &[])]);
        let roots = vec![mv("a", vec![("b", DepKind::Required)])];
        let mut installed = HashSet::new();
        installed.insert(ProjectKey::Modrinth("b".into()));
        let c = resolve_closure(&roots, &installed, &HashSet::new(), fetcher(graph))
            .await
            .unwrap();
        assert!(keys(&c).is_empty());
    }

    #[tokio::test]
    async fn skips_dep_already_installed_from_another_source_by_filename() {
        // Regression: Balm installed from Modrinth, Waystones-from-CurseForge
        // requires Balm. The CF dependency's ProjectKey never matches the
        // Modrinth install, but its jar filename does. The dep — and its whole
        // subtree — must be pruned, so no duplicate install / filename clash.
        let graph = g(&[
            ("waystones", &["balm"], &[], &[]),
            ("balm", &["balm_sub"], &[], &[]),
            ("balm_sub", &[], &[], &[]),
        ]);
        let roots = vec![mv("waystones", vec![("balm", DepKind::Required)])];
        // ProjectKey set is empty (cross-source: the installed Balm has a
        // different-source id); only the filename signals it is present.
        // `mv` names jars "<id>.jar"; the resolver lowercases before lookup.
        let mut installed_filenames = HashSet::new();
        installed_filenames.insert("balm.jar".to_string());
        let c = resolve_closure(
            &roots,
            &HashSet::new(),
            &installed_filenames,
            fetcher(graph),
        )
        .await
        .unwrap();
        assert!(
            keys(&c).is_empty(),
            "balm and its subtree must be pruned by filename, got {:?}",
            keys(&c)
        );
    }

    #[tokio::test]
    async fn loader_required_dep_is_recorded_not_installed() {
        let graph = g(&[
            ("a", &["neoforge"], &[], &["neoforge"]),
            ("neoforge", &[], &[], &[]),
        ]);
        let roots = vec![mv("a", vec![("neoforge", DepKind::Required)])];
        let c = resolve_closure(&roots, &HashSet::new(), &HashSet::new(), fetcher(graph))
            .await
            .unwrap();
        assert!(keys(&c).is_empty());
        assert!(c
            .loader_keys
            .contains(&ProjectKey::Modrinth("neoforge".into())));
    }

    #[tokio::test]
    async fn unresolvable_dep_bubbles_up_from_depth() {
        // a -> b (required); b declares an unresolvable dep referencing a
        // CurseForge mod (e.g. a Modrinth pack that can't resolve a CF ref).
        // The walk must surface that ref in closure.unresolvable even though
        // it originates two hops from the root.
        use std::future::ready;
        let fetch = move |v: ModVersion| {
            let deps = match v.project_id.as_str() {
                "a" => FetchedDeps {
                    required: vec![ResolvedNode {
                        version: mv("b", vec![]),
                        is_loader: false,
                        selection_reason: SelectionReason::NewestNoPin,
                    }],
                    ..Default::default()
                },
                "b" => FetchedDeps {
                    unresolvable: vec![DepProjectRef::Curseforge {
                        mod_id: 999,
                        file_id: None,
                    }],
                    ..Default::default()
                },
                _ => FetchedDeps::default(),
            };
            ready(Ok::<_, crate::error::Error>(deps))
        };
        let roots = vec![mv("a", vec![("b", DepKind::Required)])];
        let c = resolve_closure(&roots, &HashSet::new(), &HashSet::new(), fetch)
            .await
            .unwrap();
        assert!(
            keys(&c).contains(&"b".to_string()),
            "b must be in the required closure"
        );
        assert_eq!(
            c.unresolvable.len(),
            1,
            "exactly one unresolvable ref expected"
        );
        assert!(
            matches!(
                c.unresolvable[0],
                DepProjectRef::Curseforge { mod_id: 999, .. }
            ),
            "unresolvable ref must be the CF mod_id=999 declared by b"
        );
    }
}

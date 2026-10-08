//! Pure transitive dependency-closure resolver. Decoupled from the network:
//! callers inject an async `fetch` that, given a project key, returns that
//! project's resolved direct dependencies already classified. The walk
//! follows ONLY required edges transitively; optionals are surfaced one level
//! at a time (the caller decides whether to opt in, then re-walks from the
//! chosen optional). Cycle-safe, deduplicating, and prunes already-installed
//! projects and loader projects.
//!
//! "Already installed" has one definition here, [`InstalledView`] +
//! [`is_installed`], shared by the closure walk and by [`prune_update_deps`],
//! which applies it to an update's one-level answer.

use std::collections::{HashMap, HashSet};

use crate::mods::platform::{
    DepProjectRef, InstalledMod, ModSource, ModVersion, PlannedDep, SelectionReason,
};

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
            // TODO(vanillatweaks): placeholder — VT serves datapacks, which have
            // no Java dependency graph, so VT versions stay dead in this path.
            // A VT project_id is `<category>/<name>` and cannot collide with a
            // real Modrinth id.
            ModSource::VanillaTweaks => ProjectKey::Modrinth(v.project_id.clone()),
        }
    }
    pub fn of_ref(r: &DepProjectRef) -> ProjectKey {
        match r {
            DepProjectRef::Modrinth { project_id, .. } => ProjectKey::Modrinth(project_id.clone()),
            DepProjectRef::Curseforge { mod_id, .. } => ProjectKey::Curseforge(*mod_id),
        }
    }
    /// The key of an installed row, when it has one: a Modrinth project id, or
    /// a CurseForge numeric mod id. A row from another source, or with no
    /// platform identity at all (a hand-dropped jar), has none — pruning can
    /// then only recognise it by file name ([`InstalledView`]). Stricter than
    /// [`ProjectKey::of_version`] on purpose: that one must key every version
    /// and falls back (an unparsable CurseForge id becomes `Curseforge(0)`, the
    /// pack-only sources borrow the Modrinth tag), while a row that cannot be
    /// keyed for certain gets no key at all — the install path's rule before
    /// it moved here.
    pub fn of_installed(m: &InstalledMod) -> Option<ProjectKey> {
        ProjectKey::of_row(m.source, m.project_id.as_deref())
    }

    /// The key of a row's identity (`source`, `project_id`), when it has one — the strict rule of
    /// [`ProjectKey::of_installed`], shared with an own server's sidecar records.
    pub fn of_row(source: Option<ModSource>, project_id: Option<&str>) -> Option<ProjectKey> {
        match (source, project_id) {
            (Some(ModSource::Modrinth), Some(pid)) => Some(ProjectKey::Modrinth(pid.to_string())),
            (Some(ModSource::Curseforge), Some(pid)) => {
                pid.parse().ok().map(ProjectKey::Curseforge)
            }
            _ => None,
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

/// What an instance already has, in the two views every dependency resolver
/// prunes by. One definition for installs and updates, which had drifted: an
/// update used to install every required dependency blind
/// ([`prune_update_deps`]).
///
/// - `keys` — [`ProjectKey::of_installed`] of every row, enabled OR disabled.
///   A disabled jar of the very project a dependency names counts as present:
///   nothing installs a second copy beside it, and nothing switches it back on
///   behind the user's back. If the mod needing it cannot run without it, the
///   dependency pre-flight — which reads enabled jars only — reports it.
/// - `enabled_filenames` — the lowercased jar name of every ENABLED row. This
///   is how a dependency already satisfied from another platform (same jar,
///   another project id) or by a hand-dropped jar is recognised. A disabled
///   row stays out: `<name>.jar.disabled` neither loads nor collides with a
///   fresh `<name>.jar`, so installing the dependency beside it is right.
#[derive(Debug, Clone, Default)]
pub struct InstalledView {
    pub keys: HashSet<ProjectKey>,
    pub enabled_filenames: HashSet<String>,
}

impl InstalledView {
    pub fn of<'a, I>(rows: I) -> InstalledView
    where
        I: IntoIterator<Item = &'a InstalledMod>,
        I::IntoIter: Clone,
    {
        let rows = rows.into_iter();
        InstalledView {
            keys: rows.clone().filter_map(ProjectKey::of_installed).collect(),
            enabled_filenames: rows
                .filter(|m| m.enabled)
                .map(|m| m.filename.to_ascii_lowercase())
                .collect(),
        }
    }

    /// The same view, with the rows' identities on the other platform (spec 2026-10-08 §5): a
    /// project installed from Modrinth is present under its CurseForge id too, enabled OR
    /// disabled like its own key — a disabled copy is still a second jar of one mod id.
    pub fn with_aliases(mut self, aliases: HashSet<ProjectKey>) -> InstalledView {
        self.keys.extend(aliases);
        self
    }
}

/// What an instance has, by the one rule every install pruner and guard applies: its rows' own
/// keys and their ids on the other platform (spec 2026-10-08 aliases-everywhere, D1).
pub fn installed_view(
    rows: &[InstalledMod],
    aliases: &crate::mods::cross_ids::AliasMap,
) -> InstalledView {
    InstalledView::of(rows).with_aliases(aliases.project_keys())
}

/// Whether the instance already has `v`: its project is in `installed`, or an
/// enabled jar of the same file name is in `installed_filenames` — the views of
/// [`InstalledView`], possibly widened by a caller that is also excluding what
/// it has already collected. The one test every pruner applies:
/// [`resolve_closure`] at its frontier, [`prune_update_deps`] on an update.
pub fn is_installed(
    v: &ModVersion,
    installed: &HashSet<ProjectKey>,
    installed_filenames: &HashSet<String>,
) -> bool {
    installed.contains(&ProjectKey::of_version(v))
        || installed_filenames.contains(&v.primary_file.filename.to_ascii_lowercase())
}

/// The required dependencies an update of `target` still has to install:
/// `resolved` — the platform's one-level answer, which knows nothing of the
/// instance — minus what the instance will already have once the swap is done
/// ([`is_installed`] over an [`InstalledView`]: the install path's rule), one
/// per project, in the resolver's order.
///
/// "Once the swap is done": the outgoing row (`outgoing_sha1`) is left out of
/// the view, because the update removes that jar before it installs
/// dependencies; and `target`'s own project counts as present, as
/// [`resolve_closure`] never re-adds a root.
///
/// An update used to install every required dependency blind. One already
/// installed at another version got a second jar — two copies of one mod id,
/// and the loader refuses to start. One under the same file name with other
/// bytes failed the update with `ModsFilenameConflict` after the old jar was
/// already gone. One already current had its registry row rewritten, losing its
/// own `requires` edges. A disabled one got an enabled copy beside it.
pub fn prune_update_deps(
    registry: &[InstalledMod],
    aliases: &crate::mods::cross_ids::AliasMap,
    outgoing_sha1: &str,
    target: &ModVersion,
    resolved: &[ModVersion],
) -> Vec<ModVersion> {
    let staying: Vec<&InstalledMod> = registry
        .iter()
        .filter(|m| !m.sha1.eq_ignore_ascii_case(outgoing_sha1))
        .collect();
    // The other platform's ids of what stays, and of the target's own project: an update of the
    // Modrinth jar keeps the project its CurseForge id names.
    let target_own = (target.source, target.project_id.clone());
    let owns: Vec<(ModSource, String)> = staying
        .iter()
        .filter_map(|m| Some((m.source?, m.project_id.clone()?)))
        .collect();
    let after_swap = InstalledView::of(staying.iter().copied())
        .with_aliases(aliases.project_keys_of(owns.iter()));
    let mut seen: HashSet<ProjectKey> = HashSet::from([ProjectKey::of_version(target)]);
    seen.extend(aliases.project_keys_of(std::iter::once(&target_own)));
    resolved
        .iter()
        .filter(|v| !is_installed(v, &after_swap.keys, &after_swap.enabled_filenames))
        .filter(|v| seen.insert(ProjectKey::of_version(v)))
        .cloned()
        .collect()
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
        if visited.contains(&key) || is_installed(&p.version, installed, installed_filenames) {
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

    // ── The install path's views, shared with updates (2026-09-28) ───────────

    /// An installed row. `source: None` is a hand-dropped jar.
    fn row(
        sha1: &str,
        source: Option<ModSource>,
        project_id: Option<&str>,
        filename: &str,
        enabled: bool,
    ) -> InstalledMod {
        InstalledMod {
            filename: filename.into(),
            sha1: sha1.into(),
            source,
            project_id: project_id.map(Into::into),
            version_id: None,
            name: filename.into(),
            version_number: None,
            installed_at: "2026-09-28T00:00:00Z".into(),
            enabled,
            enrich_attempted: false,
            requires: Vec::new(),
        }
    }

    // Spec 2026-10-08 aliases-everywhere: the one view the install, the plan and the migration
    // build — a project installed from Modrinth is present under its CurseForge id too, whether
    // its jar is on or off.
    #[test]
    fn installed_view_counts_the_aliases_of_enabled_and_disabled_rows() {
        let rows = vec![
            row("on", Some(ModSource::Modrinth), Some("MJX"), "on.jar", true),
            row(
                "off",
                Some(ModSource::Modrinth),
                Some("SOD"),
                "off.jar",
                false,
            ),
        ];
        let aliases = crate::mods::cross_ids::AliasMap::from_pairs(&[
            (
                (ModSource::Curseforge, "258587".into()),
                (ModSource::Modrinth, "MJX".into()),
            ),
            (
                (ModSource::Curseforge, "394468".into()),
                (ModSource::Modrinth, "SOD".into()),
            ),
        ]);
        let view = installed_view(&rows, &aliases);
        assert!(view.keys.contains(&ProjectKey::Curseforge(258587)));
        assert!(
            view.keys.contains(&ProjectKey::Curseforge(394468)),
            "a disabled copy is a second jar of one mod id too"
        );
        assert!(view.keys.contains(&ProjectKey::Modrinth("MJX".into())));
        assert_eq!(
            view.enabled_filenames,
            HashSet::from(["on.jar".to_string()])
        );
    }

    #[test]
    fn installed_view_counts_disabled_projects_but_only_enabled_file_names() {
        // The two views `mods_install_with_deps` / `mods_resolve_install_plan`
        // built inline before they moved here — the rule an update now shares.
        let rows = vec![
            row(
                "a",
                Some(ModSource::Modrinth),
                Some("sodium"),
                "Sodium-0.6.jar",
                true,
            ),
            row(
                "b",
                Some(ModSource::Modrinth),
                Some("iris"),
                "iris-1.8.jar",
                false,
            ),
            row(
                "c",
                Some(ModSource::Curseforge),
                Some("238222"),
                "jei-19.jar",
                true,
            ),
            row(
                "d",
                Some(ModSource::Curseforge),
                Some("not-a-number"),
                "odd.jar",
                true,
            ),
            row("e", None, None, "Dropped.jar", true),
            row(
                "h",
                Some(ModSource::Hangar),
                Some("worldedit"),
                "worldedit.jar",
                true,
            ),
        ];
        let view = InstalledView::of(&rows);
        assert_eq!(
            view.keys,
            HashSet::from([
                ProjectKey::Modrinth("sodium".into()),
                ProjectKey::Modrinth("iris".into()),
                ProjectKey::Curseforge(238222),
            ])
        );
        assert_eq!(
            view.enabled_filenames,
            HashSet::from([
                "sodium-0.6.jar".to_string(),
                "jei-19.jar".to_string(),
                "odd.jar".to_string(),
                "dropped.jar".to_string(),
                "worldedit.jar".to_string(),
            ])
        );
    }

    // ── prune_update_deps ────────────────────────────────────────────────────

    /// A resolved build of `project_id` on `source`, shipped as `filename`.
    fn build(source: ModSource, project_id: &str, filename: &str) -> ModVersion {
        let mut v = mv(project_id, vec![]);
        v.source = source;
        v.primary_file.filename = filename.into();
        v
    }

    fn files(vs: &[ModVersion]) -> Vec<&str> {
        vs.iter()
            .map(|v| v.primary_file.filename.as_str())
            .collect()
    }

    /// The row being updated: Modrinth project `x`, `x-1.0.jar`, enabled.
    fn outgoing() -> InstalledMod {
        row(
            "old",
            Some(ModSource::Modrinth),
            Some("x"),
            "x-1.0.jar",
            true,
        )
    }

    fn target() -> ModVersion {
        build(ModSource::Modrinth, "x", "x-2.0.jar")
    }

    #[test]
    fn a_disabled_jar_matched_only_by_file_name_does_not_count_on_update() {
        // D1. Balm is installed from Modrinth but switched off; the dependency is
        // CurseForge's Balm — another project id, the same jar name.
        // `balm-1.0.jar.disabled` neither loads nor collides with a fresh
        // `balm-1.0.jar`, so the dependency is installed, as a fresh install does.
        let registry = vec![
            outgoing(),
            row(
                "b",
                Some(ModSource::Modrinth),
                Some("balm-mr"),
                "balm-1.0.jar",
                false,
            ),
        ];
        let resolved = [build(ModSource::Curseforge, "531761", "balm-1.0.jar")];
        assert_eq!(
            files(&prune_update_deps(
                &registry,
                &crate::mods::cross_ids::AliasMap::default(),
                "old",
                &target(),
                &resolved
            )),
            ["balm-1.0.jar"]
        );
    }

    #[test]
    fn the_jar_being_replaced_does_not_count_as_installed_on_update() {
        // D2. Two projects that ship one generic file name: the mod being updated
        // is `mod.jar`, and its new version needs another project whose jar is
        // ALSO `mod.jar`. The update removes the old jar before it installs
        // dependencies, so skipping the dependency because of that name would
        // leave the new version without it. The SHA-1 compares case-blind, as
        // every registry lookup does.
        let registry = vec![row(
            "old",
            Some(ModSource::Modrinth),
            Some("x"),
            "mod.jar",
            true,
        )];
        let resolved = [build(ModSource::Modrinth, "lib", "mod.jar")];
        assert_eq!(
            files(&prune_update_deps(
                &registry,
                &crate::mods::cross_ids::AliasMap::default(),
                "OLD",
                &target(),
                &resolved
            )),
            ["mod.jar"]
        );
    }

    // Spec 2026-10-08: the update needs CurseForge's Parasites (258587); Parasites is installed
    // from Modrinth (`srp`) and switched off — installing it would still be a second jar of one
    // mod id. And a dependency named by the target's own project on the other platform is the
    // target itself.
    #[test]
    fn a_dependency_named_by_an_installed_jars_other_id_is_not_installed_again_on_update() {
        let registry = vec![
            outgoing(),
            row(
                "p",
                Some(ModSource::Modrinth),
                Some("srp"),
                "srp-1.jar",
                false,
            ),
        ];
        let aliases = crate::mods::cross_ids::AliasMap::from_pairs(&[
            (
                (ModSource::Curseforge, "258587".into()),
                (ModSource::Modrinth, "srp".into()),
            ),
            (
                (ModSource::Curseforge, "777".into()),
                (ModSource::Modrinth, "x".into()),
            ),
        ]);
        let resolved = [
            build(ModSource::Curseforge, "258587", "srp-cf.jar"),
            build(ModSource::Curseforge, "777", "x-cf.jar"),
            build(ModSource::Modrinth, "lib", "lib.jar"),
        ];
        assert_eq!(
            files(&prune_update_deps(
                &registry,
                &aliases,
                "old",
                &target(),
                &resolved
            )),
            ["lib.jar"]
        );
    }

    #[test]
    fn the_target_and_a_repeated_project_are_installed_once_on_update() {
        // D2/D3. The resolver's answer can name the target's own project (bad
        // metadata) — the update installs the target itself, as `resolve_closure`
        // never re-adds a root — and can name one project twice: the first build
        // listed is the one installed.
        let registry = vec![outgoing()];
        let resolved = [
            build(ModSource::Modrinth, "x", "x-1.5.jar"),
            build(ModSource::Modrinth, "lib", "lib-2.0.jar"),
            build(ModSource::Modrinth, "lib", "lib-1.0.jar"),
        ];
        assert_eq!(
            files(&prune_update_deps(
                &registry,
                &crate::mods::cross_ids::AliasMap::default(),
                "old",
                &target(),
                &resolved
            )),
            ["lib-2.0.jar"]
        );
    }

    #[test]
    fn a_dependency_the_instance_lacks_is_still_installed_on_update() {
        // (pin) Pruning removes only what is there.
        let registry = vec![
            outgoing(),
            row(
                "f",
                Some(ModSource::Modrinth),
                Some("fabric-api"),
                "fabric-api-0.90.jar",
                true,
            ),
        ];
        let resolved = [build(
            ModSource::Modrinth,
            "cloth-config",
            "cloth-config-15.jar",
        )];
        assert_eq!(
            files(&prune_update_deps(
                &registry,
                &crate::mods::cross_ids::AliasMap::default(),
                "old",
                &target(),
                &resolved
            )),
            ["cloth-config-15.jar"]
        );
    }

    // ── An update end to end: snapshot → prune → swap (2026-09-28) ───────────
    //
    // `mods_update_one` takes an `AppHandle` and has no test harness, so these
    // compose the same steps it does — one registry snapshot, the pruned list to
    // install, the `requires` edges from the resolver's FULL answer, the swap,
    // the edges written last — and observe each defect on disk, through the
    // real install pipeline.
    use crate::mods::install::{install_one, update_one, ProgressCount, ProgressFn, UpdateOutcome};
    use crate::mods::installed;
    use sha1::{Digest, Sha1};
    use std::path::Path;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn nop() -> ProgressFn {
        Box::new(|_, _, _| {})
    }

    /// A build of `project_id` whose jar `filename` holds `bytes`, served by `server`.
    async fn served(
        server: &MockServer,
        source: ModSource,
        project_id: &str,
        filename: &str,
        bytes: &[u8],
    ) -> ModVersion {
        let at = format!("/{project_id}/{filename}");
        Mock::given(method("GET"))
            .and(path(at.clone()))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.to_vec()))
            .mount(server)
            .await;
        let mut v = build(source, project_id, filename);
        v.primary_file.url = format!("{}{at}", server.uri());
        v.primary_file.sha1 = Some(hex::encode(Sha1::digest(bytes)));
        v.primary_file.size = bytes.len() as f64;
        v
    }

    async fn install(dd: &Path, root: &Path, v: ModVersion, name: Option<&str>) -> String {
        install_one(dd, root, v, name.map(Into::into), &nop())
            .await
            .unwrap()
            .sha1
    }

    /// `mods_update_one` without the network resolution and the events.
    async fn update_as_the_command_does(
        dd: &Path,
        root: &Path,
        old_sha1: &str,
        target: ModVersion,
        resolved: Vec<ModVersion>,
    ) -> Result<UpdateOutcome, crate::error::Error> {
        let registry_before = installed::list(root).await?;
        let deps = prune_update_deps(
            &registry_before,
            &crate::mods::cross_ids::AliasMap::default(),
            old_sha1,
            &target,
            &resolved,
        );
        let requires = crate::mods::orphans::requires_edges(
            &registry_before,
            &InstalledView::of(&registry_before).keys,
            Some(old_sha1),
            resolved.iter(),
        );
        let outcome = update_one(
            dd,
            root,
            old_sha1,
            target,
            deps,
            &nop(),
            &ProgressCount::default(),
        )
        .await?;
        installed::set_requires(root, &outcome.primary.sha1, requires).await?;
        Ok(outcome)
    }

    #[tokio::test]
    async fn an_update_keeps_the_one_copy_of_a_dependency_installed_at_another_version() {
        // Outcome 1. Fabric API 0.90 is installed; the resolver picks 0.92 for the
        // new version of X. Installing it as well put two jars of one mod id in
        // `mods/`, and the loader refuses to start.
        let s = MockServer::start().await;
        let (dd, inst) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let x1 = served(&s, ModSource::Modrinth, "x", "x-1.0.jar", b"x one").await;
        let fapi_090 = served(
            &s,
            ModSource::Modrinth,
            "fabric-api",
            "fabric-api-0.90.jar",
            b"fapi 0.90",
        )
        .await;
        let old = install(dd.path(), inst.path(), x1, None).await;
        install(dd.path(), inst.path(), fapi_090, None).await;
        // X's install pulled Fabric API in: the edge its new row must keep (D5).
        installed::set_requires(inst.path(), &old, vec!["fabric-api".into()])
            .await
            .unwrap();

        let x2 = served(&s, ModSource::Modrinth, "x", "x-2.0.jar", b"x two").await;
        let fapi_092 = served(
            &s,
            ModSource::Modrinth,
            "fabric-api",
            "fabric-api-0.92.jar",
            b"fapi 0.92",
        )
        .await;
        let outcome = update_as_the_command_does(dd.path(), inst.path(), &old, x2, vec![fapi_092])
            .await
            .unwrap();

        let mods = installed::mods_dir(inst.path());
        assert!(
            !mods.join("fabric-api-0.92.jar").exists(),
            "a second Fabric API jar was installed"
        );
        assert!(mods.join("fabric-api-0.90.jar").exists());
        assert!(outcome.deps.is_empty());
        let rows = installed::list(inst.path()).await.unwrap();
        let fapi_rows = rows
            .iter()
            .filter(|m| m.project_id.as_deref() == Some("fabric-api"))
            .count();
        assert_eq!(fapi_rows, 1);
        let x = rows
            .iter()
            .find(|m| m.sha1 == outcome.primary.sha1)
            .unwrap();
        assert_eq!(
            x.requires,
            ["fabric-api"],
            "the edge the old row carried survives"
        );
    }

    #[tokio::test]
    async fn an_update_whose_dependency_file_name_is_taken_no_longer_fails_half_way() {
        // Outcome 2. Balm is installed from Modrinth as `balm-1.0.jar`; the new
        // version of X needs CurseForge's Balm, shipped under the same name with
        // other bytes. Installing it failed with `ModsFilenameConflict` after X's
        // old jar was already gone and the new one in — and X, which the user
        // had switched off, came back on.
        let s = MockServer::start().await;
        let (dd, inst) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let x1 = served(&s, ModSource::Modrinth, "x", "x-1.0.jar", b"x one").await;
        let balm_mr = served(
            &s,
            ModSource::Modrinth,
            "balm-mr",
            "balm-1.0.jar",
            b"balm from modrinth",
        )
        .await;
        let old = install(dd.path(), inst.path(), x1, None).await;
        install(dd.path(), inst.path(), balm_mr, None).await;
        crate::mods::install::disable(inst.path(), &old)
            .await
            .unwrap();

        let x2 = served(&s, ModSource::Modrinth, "x", "x-2.0.jar", b"x two").await;
        let balm_cf = served(
            &s,
            ModSource::Curseforge,
            "531761",
            "balm-1.0.jar",
            b"balm from curseforge",
        )
        .await;
        let outcome = update_as_the_command_does(dd.path(), inst.path(), &old, x2, vec![balm_cf])
            .await
            .expect("the update must succeed");

        let mods = installed::mods_dir(inst.path());
        assert!(
            mods.join("x-2.0.jar.disabled").exists(),
            "X stays switched off"
        );
        assert!(!mods.join("x-2.0.jar").exists());
        assert_eq!(
            std::fs::read(mods.join("balm-1.0.jar")).unwrap(),
            b"balm from modrinth"
        );
        assert!(outcome.deps.is_empty());
    }

    #[tokio::test]
    async fn an_update_leaves_the_record_of_a_current_dependency_as_it_was() {
        // Outcome 3, the commonest: the dependency is already at the version the
        // resolver picks. Re-installing it rewrote its registry row, which lost
        // its own `requires` edges and its project name.
        let s = MockServer::start().await;
        let (dd, inst) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let x1 = served(&s, ModSource::Modrinth, "x", "x-1.0.jar", b"x one").await;
        let d = served(&s, ModSource::Modrinth, "d", "d-1.0.jar", b"d one").await;
        let old = install(dd.path(), inst.path(), x1, None).await;
        let d_sha = install(dd.path(), inst.path(), d.clone(), Some("D Library")).await;
        installed::set_requires(inst.path(), &d_sha, vec!["d-lib".into()])
            .await
            .unwrap();

        let x2 = served(&s, ModSource::Modrinth, "x", "x-2.0.jar", b"x two").await;
        update_as_the_command_does(dd.path(), inst.path(), &old, x2, vec![d])
            .await
            .unwrap();

        let rows = installed::list(inst.path()).await.unwrap();
        let d_row = rows.iter().find(|m| m.sha1 == d_sha).unwrap();
        assert_eq!(d_row.requires, ["d-lib"]);
        assert_eq!(d_row.name, "D Library");
    }

    #[tokio::test]
    async fn an_update_does_not_switch_a_disabled_dependency_back_on() {
        // Outcome 4, D1. The user switched the library off. The update placed a
        // fresh, enabled copy beside `d-1.0.jar.disabled` — the library loaded
        // again — and, at the same version, replaced its row.
        let s = MockServer::start().await;
        let (dd, inst) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let _seam =
            crate::test_seam::scope(&[("LUCERNA_EXTRA_ALLOWED_HOSTS", "127.0.0.1, localhost")]);
        let x1 = served(&s, ModSource::Modrinth, "x", "x-1.0.jar", b"x one").await;
        let d = served(&s, ModSource::Modrinth, "d", "d-1.0.jar", b"d one").await;
        let old = install(dd.path(), inst.path(), x1, None).await;
        let d_sha = install(dd.path(), inst.path(), d.clone(), None).await;
        crate::mods::install::disable(inst.path(), &d_sha)
            .await
            .unwrap();

        let x2 = served(&s, ModSource::Modrinth, "x", "x-2.0.jar", b"x two").await;
        update_as_the_command_does(dd.path(), inst.path(), &old, x2, vec![d])
            .await
            .unwrap();

        let mods = installed::mods_dir(inst.path());
        assert!(
            !mods.join("d-1.0.jar").exists(),
            "an enabled copy was placed beside the disabled one"
        );
        assert!(mods.join("d-1.0.jar.disabled").exists());
        let rows = installed::list(inst.path()).await.unwrap();
        let d_rows: Vec<&InstalledMod> = rows
            .iter()
            .filter(|m| m.project_id.as_deref() == Some("d"))
            .collect();
        assert_eq!(d_rows.len(), 1);
        assert!(!d_rows[0].enabled);
    }
}

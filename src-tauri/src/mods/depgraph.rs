//! Pure nested dependency-tree builder. Network-decoupled like `deps.rs`:
//! the caller injects an async `fetch` returning a node's *direct* required
//! and optional children (already name-enriched and loader-filtered), or why
//! an installed node's are unknown. This
//! module owns recursion, cycle-guarding (per-path, keyed by source:project_id),
//! and installed/missing classification against the installed set.

use std::collections::{HashMap, HashSet};
use std::future::Future;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::mods::platform::ModSource;

/// Boxed, borrow-scoped future returned by the recursive `build_children`.
/// `async fn` cannot recurse without boxing the future, so the recursion is
/// expressed as a plain fn returning this pinned box.
type WalkFuture<'a, T> =
    std::pin::Pin<Box<dyn Future<Output = Result<T, crate::error::Error>> + Send + 'a>>;

/// Safety bounds so a pathological dependency graph can't blow up memory / the
/// IPC payload / frontend render. Real mod graphs are tiny (2–3 deep, a handful
/// of nodes), so these never bite in practice — they only cap a reconvergent or
/// adversarial graph. Beyond the depth a node is emitted but not expanded;
/// beyond the node budget, further siblings are dropped.
const MAX_DEPTH: usize = 10;
const MAX_NODES: usize = 2000;

/// Stable identity used for the installed set, dedup, and cycle detection.
pub(crate) fn key(source: ModSource, project_id: &str) -> String {
    let s = match source {
        ModSource::Modrinth => "modrinth",
        ModSource::Curseforge => "curseforge",
        ModSource::Ftb => "ftb", // FTB: pack-managed; keyed by id for dedup purposes.
        ModSource::Atlauncher => "atlauncher", // ATLauncher: pack-managed; keyed by id for dedup purposes.
        ModSource::Hangar => "hangar",
        ModSource::VanillaTweaks => "vanillatweaks", // VT: datapacks, never in a Java dep graph.
    };
    format!("{s}:{project_id}")
}

/// One installed mod, the roots of the forest.
#[derive(Debug, Clone)]
pub struct InstalledNode {
    pub sha1: String,
    pub source: ModSource,
    pub project_id: String,
    pub name: String,
}

/// A direct child edge returned by `fetch`.
#[derive(Debug, Clone)]
pub struct DepChild {
    pub source: ModSource,
    pub project_id: String,
    pub name: String,
    /// Expected jar filename of this dependency, lowercased, when the caller
    /// can derive it. Lets classification recognise a dependency satisfied by
    /// the SAME logical mod installed from the OTHER platform: the CF and
    /// Modrinth project ids differ (so `ProjectKey` never matches), but the
    /// installed jar carries the same filename. Mirrors
    /// `deps::resolve_closure`'s cross-source recognition. `None` when the
    /// filename is unknown (then only the `ProjectKey` signal applies).
    pub filename: Option<String>,
}

/// A project's direct children, as `fetch` answers them.
#[derive(Debug, Clone, Default)]
pub struct NodeDeps {
    pub required: Vec<DepChild>,
    pub optional: Vec<DepChild>,
}

/// How a declared dependency maps onto what is installed (spec 2026-10-08 §5): a project installed
/// from the other platform is the same project, under the identity it was installed by. Applied to
/// every level's children before anything else — the cycle path, the fetch memo, installed and
/// disabled — so nothing downstream keys on the declared id.
#[derive(Debug, Clone, Default)]
pub struct Canon {
    pub aliases: crate::mods::cross_ids::AliasMap,
    /// Own key → the display name the tree shows for that project.
    pub names: HashMap<String, String>,
}

impl Canon {
    /// `deps` with every child under its installed identity, each project once: within a list
    /// the first stays, and an optional child that is also required is the required one (the tree
    /// keys its items by project, and one project is one item).
    fn apply(&self, deps: &NodeDeps) -> NodeDeps {
        let to_own = |c: &DepChild| match self.aliases.own_of(c.source, &c.project_id) {
            Some((source, pid)) => DepChild {
                source: *source,
                project_id: pid.clone(),
                name: self
                    .names
                    .get(&key(*source, pid))
                    .cloned()
                    .unwrap_or_else(|| c.name.clone()),
                filename: c.filename.clone(),
            },
            None => c.clone(),
        };
        let mut seen: HashSet<String> = HashSet::new();
        let mut once = |list: &[DepChild]| -> Vec<DepChild> {
            list.iter()
                .map(&to_own)
                .filter(|c| seen.insert(key(c.source, &c.project_id)))
                .collect()
        };
        let required = once(&deps.required);
        let optional = once(&deps.optional);
        NodeDeps { required, optional }
    }
}

/// Why an installed project's dependencies are unknown. The graph says so
/// instead of showing no children: "could not tell" is never "declares
/// nothing" (CLAUDE.md, fallback discipline), and while such a mod is enabled
/// no library can be called unused.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DepsUnknown {
    /// The platform could not be asked, or did not answer: offline,
    /// rate-limited, a server error, no usable CurseForge key.
    Unreachable,
    /// The platform's answer holds no version for this jar: the registry
    /// stores no version id (an ambiguous hash match records only the
    /// project), or the platform does not list the stored one (a version
    /// removed from it; a pack-only source with no per-version data).
    Unidentified,
}

/// What `fetch` answers for one project.
#[derive(Debug, Clone)]
pub enum NodeAnswer {
    /// Its direct children. None at all is then a fact.
    Deps(NodeDeps),
    /// What its installed version declares could not be read, and why.
    /// Meaningful only for an INSTALLED project: one that is not installed is
    /// a leaf whatever the answer.
    Unknown(DepsUnknown),
}

/// The children to walk and the flag to carry: an unknown answer walks nothing.
fn split_answer(answer: NodeAnswer) -> (NodeDeps, Option<DepsUnknown>) {
    match answer {
        NodeAnswer::Deps(deps) => (deps, None),
        NodeAnswer::Unknown(why) => (NodeDeps::default(), Some(why)),
    }
}

/// What the mod's author declared on the platform. NOT a launcher verdict: the
/// loader enforces only what the jar descriptor says, and the pre-flight reads
/// that. Kept so the UI can attribute the claim, never to decide whether it is a
/// problem — the graph has no field for that, on purpose.
///
/// It replaced a four-value `DepNodeStatus` whose names (`MissingRequired`,
/// `Satisfied`) embedded the verdict in the type. A measured mod declared a
/// dependency on Modrinth that its own `neoforge.mods.toml` does not declare;
/// the loader never required it and the pack runs, so "missing required" was the
/// launcher repeating the platform's claim as its own finding.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DepDeclaration {
    Required,
    Optional,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DepTreeNode {
    pub source: ModSource,
    pub project_id: String,
    pub name: String,
    /// An ENABLED jar for this project is present in the instance.
    pub installed: bool,
    /// A registry row for this project exists but is switched off. Never set
    /// together with `installed` — an enabled jar of the project wins — and a
    /// disabled jar still satisfies nothing; the tree offers «Включить» where it
    /// would otherwise offer an install that duplicates the jar.
    /// `#[serde(default)]` so specta emits it optional.
    #[serde(default)]
    pub disabled: bool,
    pub declared: DepDeclaration,
    /// True when this project was already expanded higher on the path; its
    /// children are omitted to break cycles.
    pub cycle: bool,
    pub children: Vec<DepTreeNode>,
    /// Set on an INSTALLED node whose installed version the platform could not
    /// describe: its `children` are empty because they are unknown, not because
    /// there are none. Always `None` on a node that is not installed — a leaf by
    /// design. `#[serde(default)]` so specta emits it optional.
    #[serde(default)]
    pub deps_unknown: Option<DepsUnknown>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DepRoot {
    pub sha1: String,
    pub source: ModSource,
    pub project_id: String,
    pub name: String,
    pub required: Vec<DepTreeNode>,
    pub optional: Vec<DepTreeNode>,
    /// Why this mod's dependencies are unknown, when they are (see
    /// [`DepTreeNode::deps_unknown`]); `required` and `optional` are then empty.
    /// While such a mod is enabled, "required by nothing" is not a fact for any
    /// other mod. `#[serde(default)]` so specta emits it optional.
    #[serde(default)]
    pub deps_unknown: Option<DepsUnknown>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DependencyGraph {
    pub roots: Vec<DepRoot>,
}

/// Build the nested graph. `fetch(source, project_id)` answers that project's
/// direct children — or, for an installed project whose installed version could
/// not be read, why they are unknown ([`NodeAnswer::Unknown`], carried as
/// `deps_unknown`); memoized here so each project is fetched at most once.
///
/// `installed_filenames` (lowercased jar filenames of the installed mods) lets
/// a dependency be recognised as satisfied across sources: the same logical mod
/// installed from Modrinth but referenced as a CurseForge dependency has a
/// different `ProjectKey` yet the same jar filename. Mirrors
/// `deps::resolve_closure`'s cross-source pruning so a cross-source dependency
/// is not wrongly flagged `MissingRequired`.
///
/// `disabled` — projects whose registry rows are switched off. They only MARK a
/// node ([`DepTreeNode::disabled`]); they never satisfy it, and never root it.
pub async fn build_graph<F, Fut>(
    installed: &[InstalledNode],
    disabled: &[(ModSource, String)],
    installed_filenames: &HashSet<String>,
    canon: &Canon,
    mut fetch: F,
) -> Result<DependencyGraph, crate::error::Error>
where
    F: FnMut(ModSource, String) -> Fut + Send,
    Fut: Future<Output = Result<NodeAnswer, crate::error::Error>> + Send,
{
    let installed_keys: HashSet<String> = installed
        .iter()
        .map(|n| key(n.source, &n.project_id))
        .collect();
    let disabled_keys: HashSet<String> = disabled.iter().map(|(s, pid)| key(*s, pid)).collect();
    let mut cache: HashMap<String, NodeAnswer> = HashMap::new();
    let mut budget: usize = MAX_NODES;
    let mut roots = Vec::with_capacity(installed.len());

    for node in installed {
        let mut path: HashSet<String> = HashSet::new();
        path.insert(key(node.source, &node.project_id));
        // A root is installed by definition, so an unknown answer is its flag.
        let (deps, deps_unknown) =
            split_answer(fetch_memo(&mut cache, &mut fetch, node.source, &node.project_id).await?);
        let deps = canon.apply(&deps);
        let required = build_children(
            &deps.required,
            true,
            &installed_keys,
            &disabled_keys,
            installed_filenames,
            canon,
            &mut cache,
            &mut fetch,
            &mut path,
            1,
            &mut budget,
        )
        .await?;
        let optional = build_children(
            &deps.optional,
            false,
            &installed_keys,
            &disabled_keys,
            installed_filenames,
            canon,
            &mut cache,
            &mut fetch,
            &mut path,
            1,
            &mut budget,
        )
        .await?;
        roots.push(DepRoot {
            sha1: node.sha1.clone(),
            source: node.source,
            project_id: node.project_id.clone(),
            name: node.name.clone(),
            required,
            optional,
            deps_unknown,
        });
    }
    Ok(DependencyGraph { roots })
}

// Boxed recursion: async fns can't recurse without boxing the future.
#[allow(clippy::too_many_arguments)]
fn build_children<'a, F, Fut>(
    children: &'a [DepChild],
    required: bool,
    installed_keys: &'a HashSet<String>,
    disabled_keys: &'a HashSet<String>,
    installed_filenames: &'a HashSet<String>,
    canon: &'a Canon,
    cache: &'a mut HashMap<String, NodeAnswer>,
    fetch: &'a mut F,
    path: &'a mut HashSet<String>,
    depth: usize,
    budget: &'a mut usize,
) -> WalkFuture<'a, Vec<DepTreeNode>>
where
    F: FnMut(ModSource, String) -> Fut + Send,
    Fut: Future<Output = Result<NodeAnswer, crate::error::Error>> + Send,
{
    Box::pin(async move {
        let mut out = Vec::with_capacity(children.len());
        for c in children {
            // Node budget exhausted — stop emitting further siblings.
            if *budget == 0 {
                break;
            }
            let k = key(c.source, &c.project_id);
            // Installed if the source:project_id matches OR — for a dependency
            // satisfied cross-source — the child's known jar filename is among
            // the installed jars (its ProjectKey won't match the other-source
            // install, but the filename does). Mirrors `deps::resolve_closure`.
            let installed = installed_keys.contains(&k)
                || c.filename
                    .as_deref()
                    .is_some_and(|f| installed_filenames.contains(f));
            // Present but switched off. `installed` wins: an enabled jar of the
            // same project is the one the loader reads.
            let disabled = !installed && disabled_keys.contains(&k);
            // Two facts, carried side by side. Collapsing them into one enum is
            // what let the graph speak as if it knew a problem when it only
            // knew what the platform was told.
            let declared = if required {
                DepDeclaration::Required
            } else {
                DepDeclaration::Optional
            };
            *budget -= 1;
            // A cycle marker (its project is expanded higher on the path) and a
            // node at the depth cap are emitted but not expanded. An installed
            // one still says whether its dependencies are known — the answer is
            // memoized, so asking costs at most the one `fetch` per project.
            let cycle = path.contains(&k);
            if cycle || depth >= MAX_DEPTH {
                let deps_unknown = if installed {
                    split_answer(
                        fetch_memo(&mut *cache, &mut *fetch, c.source, &c.project_id).await?,
                    )
                    .1
                } else {
                    None
                };
                out.push(DepTreeNode {
                    source: c.source,
                    project_id: c.project_id.clone(),
                    name: c.name.clone(),
                    installed,
                    disabled,
                    declared,
                    cycle,
                    children: vec![],
                    deps_unknown,
                });
                continue;
            }
            path.insert(k.clone());
            let (deps, unknown) =
                split_answer(fetch_memo(&mut *cache, &mut *fetch, c.source, &c.project_id).await?);
            let deps = canon.apply(&deps);
            // Not installed, the node is a leaf whatever the answer — never "unknown".
            let deps_unknown = if installed { unknown } else { None };
            let req = build_children(
                &deps.required,
                true,
                installed_keys,
                disabled_keys,
                installed_filenames,
                canon,
                &mut *cache,
                &mut *fetch,
                &mut *path,
                depth + 1,
                &mut *budget,
            )
            .await?;
            let opt = build_children(
                &deps.optional,
                false,
                installed_keys,
                disabled_keys,
                installed_filenames,
                canon,
                &mut *cache,
                &mut *fetch,
                &mut *path,
                depth + 1,
                &mut *budget,
            )
            .await?;
            path.remove(&k);
            // A child node flattens its own required + optional grandchildren
            // into one `children` vec (unlike the root, which keeps them in
            // separate `required`/`optional` lists). The distinction is still
            // recoverable per node from `declared`; the UI renders nested levels
            // uniformly, so a single list is all it needs.
            let mut kids = req;
            kids.extend(opt);
            out.push(DepTreeNode {
                source: c.source,
                project_id: c.project_id.clone(),
                name: c.name.clone(),
                installed,
                disabled,
                declared,
                cycle: false,
                children: kids,
                deps_unknown,
            });
        }
        Ok(out)
    })
}

async fn fetch_memo<F, Fut>(
    cache: &mut HashMap<String, NodeAnswer>,
    fetch: &mut F,
    source: ModSource,
    project_id: &str,
) -> Result<NodeAnswer, crate::error::Error>
where
    F: FnMut(ModSource, String) -> Fut + Send,
    Fut: Future<Output = Result<NodeAnswer, crate::error::Error>> + Send,
{
    let k = key(source, project_id);
    if let Some(hit) = cache.get(&k) {
        return Ok(hit.clone());
    }
    let answer = fetch(source, project_id.to_string()).await?;
    cache.insert(k, answer.clone());
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(pid: &str, name: &str) -> DepChild {
        DepChild {
            source: ModSource::Modrinth,
            project_id: pid.into(),
            name: name.into(),
            filename: None,
        }
    }
    fn node(sha1: &str, pid: &str) -> InstalledNode {
        InstalledNode {
            sha1: sha1.into(),
            source: ModSource::Modrinth,
            project_id: pid.into(),
            name: pid.to_uppercase(),
        }
    }

    fn fetcher(
        map: std::collections::HashMap<&'static str, NodeDeps>,
    ) -> impl FnMut(ModSource, String) -> std::future::Ready<Result<NodeAnswer, crate::error::Error>>
    {
        move |_src, pid| {
            std::future::ready(Ok(NodeAnswer::Deps(
                map.get(pid.as_str()).cloned().unwrap_or_default(),
            )))
        }
    }

    /// The graph carries two independent facts and never collapses them into a
    /// verdict: whether the project is present, and what the author declared on
    /// the platform. "Is this a problem?" is the pre-flight's question — it
    /// reads the descriptor the loader enforces; the platform's word is only a
    /// claim, and a measured one contradicted its own jar.
    #[tokio::test]
    async fn a_node_carries_presence_and_the_authors_word_separately() {
        let map = std::collections::HashMap::from([(
            "rei",
            NodeDeps {
                required: vec![child("arch", "Architectury"), child("night", "Night")],
                optional: vec![child("cloth", "Cloth")],
            },
        )]);
        let installed = vec![node("r", "rei"), node("n", "night")];
        let g = build_graph(
            &installed,
            &[],
            &HashSet::new(),
            &Canon::default(),
            fetcher(map),
        )
        .await
        .unwrap();
        let rei = g.roots.iter().find(|r| r.project_id == "rei").unwrap();
        let arch = rei
            .required
            .iter()
            .find(|n| n.project_id == "arch")
            .unwrap();
        let night = rei
            .required
            .iter()
            .find(|n| n.project_id == "night")
            .unwrap();
        let cloth = rei
            .optional
            .iter()
            .find(|n| n.project_id == "cloth")
            .unwrap();

        assert!(!arch.installed);
        assert_eq!(arch.declared, DepDeclaration::Required);
        assert!(night.installed);
        assert_eq!(night.declared, DepDeclaration::Required);
        assert!(!cloth.installed);
        assert_eq!(cloth.declared, DepDeclaration::Optional);
    }

    #[tokio::test]
    async fn cross_source_dep_recognised_by_filename_not_missing() {
        // Root "waystones" (CF) requires "balm" via a CurseForge ref, but the
        // user installed Balm from Modrinth — a different ProjectKey. The jar
        // filename ("balm.jar") is what proves it is present, so the dep must
        // read as installed, not absent.
        let cf_balm = DepChild {
            source: ModSource::Curseforge,
            project_id: "531761".into(),
            name: "Balm".into(),
            filename: Some("balm.jar".into()),
        };
        let map = std::collections::HashMap::from([(
            "waystones",
            NodeDeps {
                required: vec![cf_balm],
                optional: vec![],
            },
        )]);
        // Installed set holds only the Modrinth-source Waystones root; Balm is
        // NOT in installed_keys (installed from the other source), only its
        // filename is known.
        let installed = vec![node("w", "waystones")];
        let mut filenames = HashSet::new();
        filenames.insert("balm.jar".to_string());
        let g = build_graph(&installed, &[], &filenames, &Canon::default(), fetcher(map))
            .await
            .unwrap();
        let ws = g
            .roots
            .iter()
            .find(|r| r.project_id == "waystones")
            .unwrap();
        let balm = ws
            .required
            .iter()
            .find(|n| n.project_id == "531761")
            .unwrap();
        assert!(
            balm.installed,
            "cross-source dep present by filename must read as installed"
        );
        assert_eq!(balm.declared, DepDeclaration::Required);
    }

    #[tokio::test]
    async fn optional_subtree_is_nested() {
        let map = std::collections::HashMap::from([
            (
                "rei",
                NodeDeps {
                    required: vec![],
                    optional: vec![child("cloth", "Cloth")],
                },
            ),
            (
                "cloth",
                NodeDeps {
                    required: vec![child("arch", "Architectury")],
                    optional: vec![],
                },
            ),
        ]);
        let g = build_graph(
            &[node("r", "rei")],
            &[],
            &HashSet::new(),
            &Canon::default(),
            fetcher(map),
        )
        .await
        .unwrap();
        let rei = &g.roots[0];
        let cloth = rei
            .optional
            .iter()
            .find(|n| n.project_id == "cloth")
            .unwrap();
        assert!(!cloth.installed);
        assert_eq!(cloth.declared, DepDeclaration::Optional);
        assert_eq!(cloth.children.len(), 1);
        assert_eq!(cloth.children[0].project_id, "arch");
        assert!(!cloth.children[0].installed);
        assert_eq!(cloth.children[0].declared, DepDeclaration::Required);
    }

    #[tokio::test]
    async fn cycle_guard_terminates_on_a_b_a() {
        let map = std::collections::HashMap::from([
            (
                "a",
                NodeDeps {
                    required: vec![child("b", "B")],
                    optional: vec![],
                },
            ),
            (
                "b",
                NodeDeps {
                    required: vec![child("a", "A")],
                    optional: vec![],
                },
            ),
        ]);
        let g = build_graph(
            &[node("ax", "a")],
            &[],
            &HashSet::new(),
            &Canon::default(),
            fetcher(map),
        )
        .await
        .unwrap();
        let a = &g.roots[0];
        let b = &a.required[0];
        assert_eq!(b.project_id, "b");
        let back = &b.children[0];
        assert_eq!(back.project_id, "a");
        assert!(
            back.cycle,
            "back-edge to A must be marked cycle and not expanded"
        );
        assert!(back.children.is_empty());
    }

    #[tokio::test]
    async fn diamond_is_not_treated_as_a_cycle() {
        // A→B, A→C, B→D, C→D. D is reached via two distinct paths but is NOT a
        // cycle, so it must expand fully under BOTH B and C (per-path guard,
        // not a global visited set). D requires E to prove the subtree renders.
        let map = std::collections::HashMap::from([
            (
                "a",
                NodeDeps {
                    required: vec![child("b", "B"), child("c", "C")],
                    optional: vec![],
                },
            ),
            (
                "b",
                NodeDeps {
                    required: vec![child("d", "D")],
                    optional: vec![],
                },
            ),
            (
                "c",
                NodeDeps {
                    required: vec![child("d", "D")],
                    optional: vec![],
                },
            ),
            (
                "d",
                NodeDeps {
                    required: vec![child("e", "E")],
                    optional: vec![],
                },
            ),
        ]);
        let g = build_graph(
            &[node("ax", "a")],
            &[],
            &HashSet::new(),
            &Canon::default(),
            fetcher(map),
        )
        .await
        .unwrap();
        let a = &g.roots[0];
        let b = a.required.iter().find(|n| n.project_id == "b").unwrap();
        let c = a.required.iter().find(|n| n.project_id == "c").unwrap();
        for parent in [b, c] {
            let d = &parent.children[0];
            assert_eq!(d.project_id, "d");
            assert!(
                !d.cycle,
                "D via {} must not be flagged as a cycle",
                parent.project_id
            );
            assert_eq!(
                d.children[0].project_id, "e",
                "D's subtree must expand under both parents"
            );
        }
    }

    #[tokio::test]
    async fn caps_recursion_depth_on_a_long_chain() {
        // a0 → a1 → … → a13, a chain longer than MAX_DEPTH. The walk must stop
        // expanding at the cap rather than recurse the whole chain.
        let fetch = |_src: ModSource, pid: String| {
            let deps = pid
                .strip_prefix('a')
                .and_then(|n| n.parse::<u32>().ok())
                .filter(|n| *n < 13)
                .map(|n| NodeDeps {
                    required: vec![child(&format!("a{}", n + 1), &format!("A{}", n + 1))],
                    optional: vec![],
                })
                .unwrap_or_default();
            std::future::ready(Ok(NodeAnswer::Deps(deps)))
        };
        let g = build_graph(
            &[node("r", "a0")],
            &[],
            &HashSet::new(),
            &Canon::default(),
            fetch,
        )
        .await
        .unwrap();

        fn depth(nodes: &[DepTreeNode]) -> usize {
            nodes
                .iter()
                .map(|n| 1 + depth(&n.children))
                .max()
                .unwrap_or(0)
        }
        let d = depth(&g.roots[0].required);
        assert_eq!(
            d, MAX_DEPTH,
            "a chain longer than the cap must be capped at MAX_DEPTH"
        );
    }

    /// r → x (disabled) → y → x: the normal literal and the cycle literal.
    #[tokio::test]
    async fn a_disabled_project_is_marked_disabled_and_never_installed() {
        let map = HashMap::from([
            (
                "r",
                NodeDeps {
                    required: vec![child("x", "X")],
                    optional: vec![],
                },
            ),
            (
                "x",
                NodeDeps {
                    required: vec![child("y", "Y")],
                    optional: vec![],
                },
            ),
            (
                "y",
                NodeDeps {
                    required: vec![child("x", "X")],
                    optional: vec![],
                },
            ),
        ]);
        let disabled = [(ModSource::Modrinth, "x".to_string())];
        let g = build_graph(
            &[node("rs", "r")],
            &disabled,
            &HashSet::new(),
            &Canon::default(),
            fetcher(map),
        )
        .await
        .unwrap();
        let x = &g.roots[0].required[0];
        assert!(x.disabled && !x.installed, "switched off is not installed");
        let y = &x.children[0];
        assert!(!y.disabled && !y.installed);
        let back = &y.children[0];
        assert!(
            back.cycle && back.disabled,
            "the cycle marker carries the flag too"
        );
    }

    #[tokio::test]
    async fn a_disabled_node_at_the_depth_cap_is_still_marked() {
        let fetch = |_src: ModSource, pid: String| {
            let deps = pid
                .strip_prefix('a')
                .and_then(|n| n.parse::<u32>().ok())
                .filter(|n| *n < 13)
                .map(|n| NodeDeps {
                    required: vec![child(&format!("a{}", n + 1), "A")],
                    optional: vec![],
                })
                .unwrap_or_default();
            std::future::ready(Ok(NodeAnswer::Deps(deps)))
        };
        let disabled: Vec<(ModSource, String)> = (1..=13)
            .map(|n| (ModSource::Modrinth, format!("a{n}")))
            .collect();
        let g = build_graph(
            &[node("r", "a0")],
            &disabled,
            &HashSet::new(),
            &Canon::default(),
            fetch,
        )
        .await
        .unwrap();
        let mut n = &g.roots[0].required[0];
        while let Some(next) = n.children.first() {
            n = next;
        }
        assert_eq!(n.project_id, "a10", "the walk stops at the depth cap");
        assert!(n.disabled, "the depth-capped leaf is disabled");
    }

    #[tokio::test]
    async fn an_enabled_jar_of_the_same_project_wins_over_a_disabled_one() {
        let map = HashMap::from([(
            "rei",
            NodeDeps {
                required: vec![child("night", "Night")],
                optional: vec![],
            },
        )]);
        let disabled = [(ModSource::Modrinth, "night".to_string())];
        let installed = vec![node("r", "rei"), node("n", "night")];
        let g = build_graph(
            &installed,
            &disabled,
            &HashSet::new(),
            &Canon::default(),
            fetcher(map),
        )
        .await
        .unwrap();
        let rei = g.roots.iter().find(|r| r.project_id == "rei").unwrap();
        assert!(rei.required[0].installed && !rei.required[0].disabled);
    }

    /// "Could not tell" is not "declares nothing": an installed project whose
    /// installed version the platform could not describe says so — as a root and
    /// nested — while a project that is not installed stays a plain leaf whatever
    /// the answer.
    #[tokio::test]
    async fn an_installed_project_the_platform_could_not_describe_is_unknown_not_a_leaf() {
        let fetch = |_src: ModSource, pid: String| {
            let answer = match pid.as_str() {
                "r" => NodeAnswer::Deps(NodeDeps {
                    required: vec![child("x", "X"), child("y", "Y")],
                    optional: vec![],
                }),
                "x" => NodeAnswer::Unknown(DepsUnknown::Unidentified),
                "y" | "q" => NodeAnswer::Unknown(DepsUnknown::Unreachable),
                _ => NodeAnswer::Deps(NodeDeps::default()),
            };
            std::future::ready(Ok(answer))
        };
        let installed = vec![node("rs", "r"), node("xs", "x"), node("qs", "q")];
        let g = build_graph(&installed, &[], &HashSet::new(), &Canon::default(), fetch)
            .await
            .unwrap();
        let root = |pid: &str| g.roots.iter().find(|r| r.project_id == pid).unwrap();

        assert_eq!(root("q").deps_unknown, Some(DepsUnknown::Unreachable));
        assert!(root("q").required.is_empty() && root("q").optional.is_empty());
        assert_eq!(root("r").deps_unknown, None, "r's version was described");
        let x = &root("r").required[0];
        assert!(x.installed);
        assert_eq!(
            x.deps_unknown,
            Some(DepsUnknown::Unidentified),
            "nested, the flag rides along"
        );
        let y = &root("r").required[1];
        assert!(!y.installed);
        assert_eq!(
            y.deps_unknown, None,
            "a project that is not installed is a leaf, never unknown"
        );
    }

    /// The depth cap stops the walk, not the flag: an installed node emitted
    /// unexpanded at the cap still says its dependencies are unknown.
    #[tokio::test]
    async fn an_installed_node_at_the_depth_cap_still_says_its_dependencies_are_unknown() {
        let fetch = |_src: ModSource, pid: String| {
            let answer = match pid.as_str() {
                "a10" => NodeAnswer::Unknown(DepsUnknown::Unreachable),
                _ => NodeAnswer::Deps(
                    pid.strip_prefix('a')
                        .and_then(|n| n.parse::<u32>().ok())
                        .filter(|n| *n < 13)
                        .map(|n| NodeDeps {
                            required: vec![child(&format!("a{}", n + 1), "A")],
                            optional: vec![],
                        })
                        .unwrap_or_default(),
                ),
            };
            std::future::ready(Ok(answer))
        };
        let installed = vec![node("r", "a0"), node("t", "a10")];
        let g = build_graph(&installed, &[], &HashSet::new(), &Canon::default(), fetch)
            .await
            .unwrap();
        let mut n = &g.roots[0].required[0];
        while let Some(next) = n.children.first() {
            n = next;
        }
        assert_eq!(n.project_id, "a10", "the walk stops at the depth cap");
        assert!(n.installed);
        assert_eq!(n.deps_unknown, Some(DepsUnknown::Unreachable));
    }

    #[test]
    fn a_node_serialized_before_the_field_reads_as_not_disabled() {
        let n: DepTreeNode = serde_json::from_value(serde_json::json!({
            "source": "modrinth", "project_id": "a", "name": "A", "installed": false,
            "declared": "required", "cycle": false, "children": []
        }))
        .unwrap();
        assert!(!n.disabled);
    }

    // Spec 2026-10-08: an addon from CurseForge names the main mod by its CurseForge id; the main
    // mod is installed from Modrinth. Through the alias it is that installed project.
    fn cf_child(id: &str, name: &str) -> DepChild {
        DepChild {
            source: ModSource::Curseforge,
            project_id: id.into(),
            name: name.into(),
            filename: None,
        }
    }
    fn alias_canon(pairs: &[(&str, &str)], names: &[(&str, &str)]) -> Canon {
        let pairs: Vec<_> = pairs
            .iter()
            .map(|(cf, mr)| {
                (
                    (ModSource::Curseforge, cf.to_string()),
                    (ModSource::Modrinth, mr.to_string()),
                )
            })
            .collect();
        Canon {
            aliases: crate::mods::cross_ids::AliasMap::from_pairs(&pairs),
            names: names
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[tokio::test]
    async fn a_dependency_declared_by_the_other_platforms_id_is_the_installed_project() {
        let map = std::collections::HashMap::from([
            (
                "addon",
                NodeDeps {
                    required: vec![],
                    optional: vec![cf_child("258587", "Parasites on CurseForge")],
                },
            ),
            (
                "srp",
                NodeDeps {
                    required: vec![child("lib", "Lib")],
                    optional: vec![],
                },
            ),
        ]);
        let installed = vec![node("a", "addon"), node("s", "srp")];
        let canon = alias_canon(
            &[("258587", "srp")],
            &[("modrinth:srp", "Scape and Run: Parasites")],
        );
        let g = build_graph(&installed, &[], &HashSet::new(), &canon, fetcher(map))
            .await
            .unwrap();
        let n = &g.roots[0].optional[0];
        assert_eq!(
            (n.source, n.project_id.as_str()),
            (ModSource::Modrinth, "srp")
        );
        assert_eq!(n.name, "Scape and Run: Parasites");
        assert!(n.installed && !n.disabled);
        assert_eq!(
            n.children.len(),
            1,
            "its own dependencies are read, never a leaf"
        );
        assert_eq!(n.children[0].project_id, "lib");
    }

    #[tokio::test]
    async fn a_way_back_to_the_root_through_an_alias_is_a_cycle() {
        let map = std::collections::HashMap::from([
            (
                "srp",
                NodeDeps {
                    required: vec![],
                    optional: vec![child("addon", "Addon")],
                },
            ),
            (
                "addon",
                NodeDeps {
                    required: vec![],
                    optional: vec![cf_child("258587", "P")],
                },
            ),
        ]);
        let installed = vec![node("s", "srp"), node("a", "addon")];
        let canon = alias_canon(&[("258587", "srp")], &[]);
        let g = build_graph(&installed, &[], &HashSet::new(), &canon, fetcher(map))
            .await
            .unwrap();
        let back = &g.roots[0].optional[0].children[0];
        assert_eq!(back.project_id, "srp");
        assert!(
            back.cycle,
            "the root met again under its other id is a cycle"
        );
    }

    #[tokio::test]
    async fn an_alias_of_a_switched_off_jar_marks_the_node_disabled_never_installed() {
        let map = std::collections::HashMap::from([(
            "addon",
            NodeDeps {
                required: vec![cf_child("258587", "P")],
                optional: vec![],
            },
        )]);
        let disabled = [(ModSource::Modrinth, "srp".to_string())];
        let canon = alias_canon(&[("258587", "srp")], &[]);
        let g = build_graph(
            &[node("a", "addon")],
            &disabled,
            &HashSet::new(),
            &canon,
            fetcher(map),
        )
        .await
        .unwrap();
        let n = &g.roots[0].required[0];
        assert_eq!(n.project_id, "srp");
        assert!(n.disabled && !n.installed);
    }

    #[tokio::test]
    async fn one_project_declared_twice_is_one_node_and_required_wins() {
        let map = std::collections::HashMap::from([(
            "addon",
            NodeDeps {
                required: vec![cf_child("258587", "P")],
                optional: vec![child("srp", "SRP"), child("srp", "SRP")],
            },
        )]);
        let installed = vec![node("a", "addon"), node("s", "srp")];
        let canon = alias_canon(&[("258587", "srp")], &[]);
        let g = build_graph(&installed, &[], &HashSet::new(), &canon, fetcher(map))
            .await
            .unwrap();
        assert_eq!(g.roots[0].required.len(), 1);
        assert!(g.roots[0].optional.is_empty());
    }
}

//! Pure nested dependency-tree builder. Network-decoupled like `deps.rs`:
//! the caller injects an async `fetch` returning a node's *direct* required
//! and optional children (already name-enriched and loader-filtered). This
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

/// Stable identity used for the installed set, dedup, and cycle detection.
fn key(source: ModSource, project_id: &str) -> String {
    let s = match source {
        ModSource::Modrinth => "modrinth",
        ModSource::Curseforge => "curseforge",
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
}

/// What `fetch` returns for one project.
#[derive(Debug, Clone, Default)]
pub struct NodeDeps {
    pub required: Vec<DepChild>,
    pub optional: Vec<DepChild>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DepNodeStatus {
    Satisfied,
    MissingRequired,
    OptionalPresent,
    OptionalAbsent,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DepTreeNode {
    pub source: ModSource,
    pub project_id: String,
    pub name: String,
    pub status: DepNodeStatus,
    /// True when this project was already expanded higher on the path; its
    /// children are omitted to break cycles.
    pub cycle: bool,
    pub children: Vec<DepTreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DepRoot {
    pub sha1: String,
    pub source: ModSource,
    pub project_id: String,
    pub name: String,
    pub required: Vec<DepTreeNode>,
    pub optional: Vec<DepTreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DependencyGraph {
    pub roots: Vec<DepRoot>,
}

/// Build the nested graph. `fetch(source, project_id)` returns that project's
/// direct children; memoized here so each project is fetched at most once.
pub async fn build_graph<F, Fut>(
    installed: &[InstalledNode],
    mut fetch: F,
) -> Result<DependencyGraph, crate::error::Error>
where
    F: FnMut(ModSource, String) -> Fut + Send,
    Fut: Future<Output = Result<NodeDeps, crate::error::Error>> + Send,
{
    let installed_keys: HashSet<String> = installed
        .iter()
        .map(|n| key(n.source, &n.project_id))
        .collect();
    let mut cache: HashMap<String, NodeDeps> = HashMap::new();
    let mut roots = Vec::with_capacity(installed.len());

    for node in installed {
        let mut path: HashSet<String> = HashSet::new();
        path.insert(key(node.source, &node.project_id));
        let deps = fetch_memo(&mut cache, &mut fetch, node.source, &node.project_id).await?;
        let required = build_children(
            &deps.required,
            true,
            &installed_keys,
            &mut cache,
            &mut fetch,
            &mut path,
        )
        .await?;
        let optional = build_children(
            &deps.optional,
            false,
            &installed_keys,
            &mut cache,
            &mut fetch,
            &mut path,
        )
        .await?;
        roots.push(DepRoot {
            sha1: node.sha1.clone(),
            source: node.source,
            project_id: node.project_id.clone(),
            name: node.name.clone(),
            required,
            optional,
        });
    }
    Ok(DependencyGraph { roots })
}

// Boxed recursion: async fns can't recurse without boxing the future.
fn build_children<'a, F, Fut>(
    children: &'a [DepChild],
    required: bool,
    installed_keys: &'a HashSet<String>,
    cache: &'a mut HashMap<String, NodeDeps>,
    fetch: &'a mut F,
    path: &'a mut HashSet<String>,
) -> WalkFuture<'a, Vec<DepTreeNode>>
where
    F: FnMut(ModSource, String) -> Fut + Send,
    Fut: Future<Output = Result<NodeDeps, crate::error::Error>> + Send,
{
    Box::pin(async move {
        let mut out = Vec::with_capacity(children.len());
        for c in children {
            let k = key(c.source, &c.project_id);
            let installed = installed_keys.contains(&k);
            let status = match (required, installed) {
                (true, true) => DepNodeStatus::Satisfied,
                (true, false) => DepNodeStatus::MissingRequired,
                (false, true) => DepNodeStatus::OptionalPresent,
                (false, false) => DepNodeStatus::OptionalAbsent,
            };
            if path.contains(&k) {
                out.push(DepTreeNode {
                    source: c.source,
                    project_id: c.project_id.clone(),
                    name: c.name.clone(),
                    status,
                    cycle: true,
                    children: vec![],
                });
                continue;
            }
            path.insert(k.clone());
            let deps = fetch_memo(&mut *cache, &mut *fetch, c.source, &c.project_id).await?;
            let req = build_children(
                &deps.required,
                true,
                installed_keys,
                &mut *cache,
                &mut *fetch,
                &mut *path,
            )
            .await?;
            let opt = build_children(
                &deps.optional,
                false,
                installed_keys,
                &mut *cache,
                &mut *fetch,
                &mut *path,
            )
            .await?;
            path.remove(&k);
            // A child node flattens its own required + optional grandchildren
            // into one `children` vec (unlike the root, which keeps them in
            // separate `required`/`optional` lists). The distinction is still
            // recoverable per node from `status`; the UI renders nested levels
            // uniformly, so a single list is all it needs.
            let mut kids = req;
            kids.extend(opt);
            out.push(DepTreeNode {
                source: c.source,
                project_id: c.project_id.clone(),
                name: c.name.clone(),
                status,
                cycle: false,
                children: kids,
            });
        }
        Ok(out)
    })
}

async fn fetch_memo<F, Fut>(
    cache: &mut HashMap<String, NodeDeps>,
    fetch: &mut F,
    source: ModSource,
    project_id: &str,
) -> Result<NodeDeps, crate::error::Error>
where
    F: FnMut(ModSource, String) -> Fut + Send,
    Fut: Future<Output = Result<NodeDeps, crate::error::Error>> + Send,
{
    let k = key(source, project_id);
    if let Some(hit) = cache.get(&k) {
        return Ok(hit.clone());
    }
    let deps = fetch(source, project_id.to_string()).await?;
    cache.insert(k, deps.clone());
    Ok(deps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(pid: &str, name: &str) -> DepChild {
        DepChild {
            source: ModSource::Modrinth,
            project_id: pid.into(),
            name: name.into(),
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
    ) -> impl FnMut(ModSource, String) -> std::future::Ready<Result<NodeDeps, crate::error::Error>>
    {
        move |_src, pid| std::future::ready(Ok(map.get(pid.as_str()).cloned().unwrap_or_default()))
    }

    #[tokio::test]
    async fn classifies_missing_required_and_satisfied() {
        let map = std::collections::HashMap::from([(
            "rei",
            NodeDeps {
                required: vec![child("arch", "Architectury"), child("night", "Night")],
                optional: vec![],
            },
        )]);
        let installed = vec![node("r", "rei"), node("n", "night")];
        let g = build_graph(&installed, fetcher(map)).await.unwrap();
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
        assert_eq!(arch.status, DepNodeStatus::MissingRequired);
        assert_eq!(night.status, DepNodeStatus::Satisfied);
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
        let g = build_graph(&[node("r", "rei")], fetcher(map))
            .await
            .unwrap();
        let rei = &g.roots[0];
        let cloth = rei
            .optional
            .iter()
            .find(|n| n.project_id == "cloth")
            .unwrap();
        assert_eq!(cloth.status, DepNodeStatus::OptionalAbsent);
        assert_eq!(cloth.children.len(), 1);
        assert_eq!(cloth.children[0].project_id, "arch");
        assert_eq!(cloth.children[0].status, DepNodeStatus::MissingRequired);
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
        let g = build_graph(&[node("ax", "a")], fetcher(map)).await.unwrap();
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
        let g = build_graph(&[node("ax", "a")], fetcher(map)).await.unwrap();
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
}

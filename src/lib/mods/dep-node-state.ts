import type { TranslationKey } from '$lib/i18n/keys.generated';
import type {
  DepProjectKey,
  DepProjectRef,
  DepsUnknown,
  DepTreeNode,
  DepViolation,
  PreflightReport,
} from '$lib/ipc/bindings';
import { isRangeRemediable } from './violation-view';

// What the dependency tree may truthfully say about a node (spec 2026-09-28 §6.3, D4). The
// platform's dependency list is the AUTHOR speaking; only the pre-flight — the jar descriptor the
// loader opens — says whether the game starts without it. So an absent required node is red ONLY
// when this dependent's own `missing_required` violation resolves to this very project
// (dependent-scoped, audit A-F3), and "starts without it" ONLY when the dependent was judged and
// the loader asked for nothing. Everything else is neutral and claims nothing. A version mismatch
// is dependent-scoped the same way (`edgeConflict`): a node is out of range under the mod whose
// own range rejects it, never under every mod that shows it.

export type DepNodeState =
  | 'out_of_range'
  | 'installed'
  | 'disabled'
  | 'optional_absent'
  | 'loader_required'
  | 'platform_only'
  | 'unknown';

/** What the Installed tab threads into its trees. */
export type DepTreeCtx = {
  readonly report: PreflightReport | null;
  projectOf: (dependentSha1: string, depId: string) => DepProjectKey | null;
  /** The enabled row's sha1 for a `source:project_id` key — a nested node's dependent. */
  enabledShaOf: (projectKey: string) => string | null;
  /** Switch a disabled dependency back on (the host looks its jar up by project). */
  onEnable: (node: DepTreeNode) => void;
  /**
   * The version conflict an out-of-range node stands for — `dependentSha1`'s (this level's
   * dependent) own, `edgeConflict` over the violations the host can fix — or null when the host
   * has none to fix, and the node offers no «Fix…».
   */
  conflictOf: (node: DepTreeNode, dependentSha1: string | null) => DepViolation | null;
  /** Ask the planner about that conflict (spec §6.5); its offers show in the host's panel. */
  onPlan: (conflict: DepViolation) => void;
};

export const EMPTY_TREE_CTX: DepTreeCtx = {
  report: null,
  projectOf: () => null,
  enabledShaOf: () => null,
  onEnable: () => {},
  conflictOf: () => null,
  onPlan: () => {},
};

// An installed mod whose installed version the platform could not describe has no children
// because they are unknown, not because there are none — the tree says so, and why (a missing
// reason is a compile error).
export const DEPS_UNKNOWN_KEY: Record<DepsUnknown, TranslationKey> = {
  unreachable: 'mods.deps.depsUnknownUnreachable',
  unidentified: 'mods.deps.depsUnknownUnidentified',
};

/**
 * The version conflict on the edge `dependentSha1` → `node`: the dependent's OWN range violation
 * whose provider is this node's project, or null. Per edge, never per project — Indium's range on
 * Sodium is Indium's conflict, and Sodium under Iris, which accepts it, is simply installed. The
 * provider is the jar the pre-flight measured the range against: `provider_project` is that jar's
 * platform link (its registry source and project), so a provider with no link marks no node — the
 * tree knows a node by its project only. An incompatibility is no range to satisfy and marks
 * nothing (`isRangeRemediable`).
 */
export function edgeConflict(
  violations: readonly DepViolation[],
  node: { source: string; project_id: string },
  dependentSha1: string | null,
): DepViolation | null {
  if (dependentSha1 === null) return null;
  const key = `${node.source}:${node.project_id}`;
  return (
    violations.find(
      (v) =>
        v.dependent_sha1 === dependentSha1 &&
        isRangeRemediable(v) &&
        v.provider_project !== null &&
        projectRefKey(v.provider_project) === key,
    ) ?? null
  );
}

// A node's key is `source:project_id`; a CurseForge node keeps its numeric mod id as the project id.
function projectRefKey(ref: DepProjectRef): string {
  return ref.source === 'modrinth' ? `modrinth:${ref.project_id}` : `curseforge:${ref.mod_id}`;
}

export function classifyDepNode(i: {
  node: DepTreeNode;
  /** The enabled jar that declared this edge; null = none (absent parent). */
  dependentSha1: string | null;
  report: PreflightReport | null;
  projectOf: DepTreeCtx['projectOf'];
}): DepNodeState {
  const { node } = i;
  if (edgeConflict(i.report?.violations ?? [], node, i.dependentSha1)) return 'out_of_range';
  if (node.installed) return 'installed';
  if (node.disabled) return 'disabled';
  if (node.declared === 'optional') return 'optional_absent';
  const sha = i.dependentSha1;
  if (sha === null || i.report === null) return 'unknown';
  // Not judged (jar missing or unreadable): its silence is not "asked for nothing".
  if ((i.report.unjudged ?? []).includes(sha)) return 'unknown';
  const missing = i.report.violations.filter(
    (v) => v.dependent_sha1 === sha && v.kind === 'missing_required',
  );
  if (missing.length === 0) return 'platform_only';
  // It misses something — whether THIS project is that something, only a resolved name can say.
  const pointsHere = missing.some((v) => {
    const p = i.projectOf(sha, v.dep_id);
    return p !== null && p.source === node.source && p.project_id === node.project_id;
  });
  return pointsHere ? 'loader_required' : 'unknown';
}

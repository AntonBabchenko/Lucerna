import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { DepProjectKey, DepsUnknown, DepTreeNode, PreflightReport } from '$lib/ipc/bindings';

// What the dependency tree may truthfully say about a node (spec 2026-09-28 §6.3, D4). The
// platform's dependency list is the AUTHOR speaking; only the pre-flight — the jar descriptor the
// loader opens — says whether the game starts without it. So an absent required node is red ONLY
// when this dependent's own `missing_required` violation resolves to this very project
// (dependent-scoped, audit A-F3), and "starts without it" ONLY when the dependent was judged and
// the loader asked for nothing. Everything else is neutral and claims nothing.

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
};

export const EMPTY_TREE_CTX: DepTreeCtx = {
  report: null,
  projectOf: () => null,
  enabledShaOf: () => null,
  onEnable: () => {},
};

// An installed mod whose installed version the platform could not describe has no children
// because they are unknown, not because there are none — the tree says so, and why (a missing
// reason is a compile error).
export const DEPS_UNKNOWN_KEY: Record<DepsUnknown, TranslationKey> = {
  unreachable: 'mods.deps.depsUnknownUnreachable',
  unidentified: 'mods.deps.depsUnknownUnidentified',
};

export function classifyDepNode(i: {
  node: DepTreeNode;
  /** The enabled jar that declared this edge; null = none (absent parent). */
  dependentSha1: string | null;
  report: PreflightReport | null;
  projectOf: DepTreeCtx['projectOf'];
  outOfRange: boolean;
}): DepNodeState {
  const { node } = i;
  if (i.outOfRange) return 'out_of_range';
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

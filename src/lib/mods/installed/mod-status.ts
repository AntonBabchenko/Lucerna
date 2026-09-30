import type { DepViolation, ModUpdateState, ViolationKind } from '$lib/ipc/bindings';
import type { CompatHint } from './compat-check.svelte';

/**
 * One status per installed row — the single problem model the Installed tab, the Overview and the
 * Play gate share (spec 2026-09-28 §6.2). Pure.
 *
 * - blocking: any pre-flight violation where this mod is the dependent. The pre-flight emits
 *   `platform_mismatch` from the same verdict the compat scan reads, so it is the ONE source for
 *   that fact (audit C-Q4); compat's platform hints are ignored here.
 * - warning: compat only — a jar for another loader family (the loader skips it; the game still
 *   starts) or a live «no release» verdict. Also every pre-flight reason when the caller marks the
 *   report `advisory` (a self-completing pack still has files to download — `hasBlocking`'s rule).
 * - disabled: the loader never reads a disabled jar and the pre-flight does not judge it, so that
 *   is all there is to say.
 * - held / update / ok: nothing is wrong; a hold hides an available update.
 */
export type StatusLevel = 'blocking' | 'warning' | 'update' | 'held' | 'disabled' | 'ok';

type CompatWarning = Extract<CompatHint, { key: 'loader' } | { key: 'noRelease' }>;

export type StatusReason =
  | { source: 'preflight'; violation: DepViolation }
  | { source: 'compat'; hint: CompatWarning };

/** The reason's one fix (the row shows it; `fixAll` runs the automatic ones). */
export type StatusFix =
  | { kind: 'enable-dependency'; violation: DepViolation }
  | { kind: 'install-dependency'; violation: DepViolation }
  | { kind: 'fix-version'; violation: DepViolation }
  | { kind: 'choose-version'; violation: DepViolation }
  | { kind: 'migrate' }
  | { kind: 'update' };

export type ModStatus = { level: StatusLevel; reasons: StatusReason[]; fix: StatusFix | null };

export type StatusInput = {
  enabled: boolean;
  /** Pre-flight violations where THIS mod is the dependent. */
  violations: readonly DepViolation[];
  /** `compat.hintFor(sha1)` for a mod compat counts as incompatible, else null. */
  compat: CompatHint | null;
  /** This mod's update-check state, or null when unchecked. */
  update: ModUpdateState | null;
  /** The project is held: no updates offered. */
  held: boolean;
  /**
   * Every pre-flight reason is advisory: a self-completing pack is still downloading its files
   * (`hasBlocking`'s rule). A caller that passes only `hasBlocking` violations leaves it unset.
   */
  advisory?: boolean;
};

// The jar itself first (a jar built for another platform makes its dependency complaints moot),
// then what stops the load outright, then what a version change fixes. A Record, so a new kind is
// a compile error here rather than an unranked reason.
const KIND_RANK: Record<ViolationKind, number> = {
  platform_mismatch: 0,
  missing_required: 1,
  required_disabled: 2,
  version_out_of_range: 3,
  optional_out_of_range: 4,
  incompatible_installed: 5,
};

function byRank(a: DepViolation, b: DepViolation): number {
  const rank = KIND_RANK[a.kind] - KIND_RANK[b.kind];
  if (rank !== 0) return rank;
  // Code-unit order, not the UI locale's: the same report always lists the same way.
  return a.dep_id < b.dep_id ? -1 : a.dep_id > b.dep_id ? 1 : 0;
}

function fixFor(violation: DepViolation): StatusFix {
  switch (violation.kind) {
    case 'required_disabled':
      return { kind: 'enable-dependency', violation };
    case 'missing_required':
      return { kind: 'install-dependency', violation };
    case 'platform_mismatch':
      // Needs another build of this mod or the migration plan — never automatic (spec §6.4).
      return { kind: 'choose-version', violation };
    case 'version_out_of_range':
    case 'optional_out_of_range':
    case 'incompatible_installed':
      return { kind: 'fix-version', violation };
  }
}

function compatWarning(hint: CompatHint | null): CompatWarning | null {
  if (hint === null) return null;
  switch (hint.key) {
    case 'loader':
    case 'noRelease':
      return hint;
    case 'platformMc':
    case 'platformLoader':
      // The pre-flight reports this jar as `platform_mismatch` — the one source for that fact.
      return null;
  }
}

export function statusOf(input: StatusInput): ModStatus {
  if (!input.enabled) return { level: 'disabled', reasons: [], fix: null };
  // A copy: the caller's list (often frozen $state) is never sorted in place.
  const pre = [...input.violations].sort(byRank);
  const hint = compatWarning(input.compat);
  const reasons: StatusReason[] = [
    ...pre.map((violation): StatusReason => ({ source: 'preflight', violation })),
    ...(hint ? [{ source: 'compat' as const, hint }] : []),
  ];
  const first = pre[0];
  if (first) {
    return { level: input.advisory ? 'warning' : 'blocking', reasons, fix: fixFor(first) };
  }
  if (hint) return { level: 'warning', reasons, fix: { kind: 'migrate' } };
  if (input.held) return { level: 'held', reasons: [], fix: null };
  if (input.update?.kind === 'update_available') {
    return { level: 'update', reasons: [], fix: { kind: 'update' } };
  }
  return { level: 'ok', reasons: [], fix: null };
}

/** The «Проблемы» chip counts exactly these: blocking ∪ warning. */
export function isProblem(status: ModStatus | undefined): boolean {
  return status?.level === 'blocking' || status?.level === 'warning';
}

/**
 * How many mods stop the game and how many may not work — the Overview's numbers (spec §6.2),
 * without the rows: each mod the pre-flight or compat names gets the level `statusOf` gives its
 * Installed row, from the same inputs. `violations` are the BLOCKING ones (`hasBlocking` applied
 * by the caller, as the Installed tab does); `hints` are compat's, by sha1. Both judge enabled
 * mods only, so every mod they name is enabled.
 */
export function problemCounts(
  violations: readonly DepViolation[],
  hints: ReadonlyMap<string, CompatHint>,
): { blocking: number; warning: number } {
  const bySha = new Map<string, DepViolation[]>();
  for (const v of violations) {
    bySha.set(v.dependent_sha1, [...(bySha.get(v.dependent_sha1) ?? []), v]);
  }
  let blocking = 0;
  let warning = 0;
  for (const sha of new Set([...bySha.keys(), ...hints.keys()])) {
    const { level } = statusOf({
      enabled: true,
      violations: bySha.get(sha) ?? [],
      compat: hints.get(sha) ?? null,
      update: null,
      held: false,
    });
    if (level === 'blocking') blocking++;
    else if (level === 'warning') warning++;
  }
  return { blocking, warning };
}

// The dependencies tour of the Installed list (spec 2026-10-08-deps-tour): which mod it shows,
// which of its steps exist, and when it may start. Pure — InstalledModsView feeds it.

import type { DepRoot, ModSource } from '$lib/ipc/bindings';
import { DEPS_STEPS } from '$lib/onboarding/contextual-tours';
import type { TourStep } from '$lib/onboarding/steps';

/** A row of the current page, as the tour needs it. */
export type DepsTourRow = {
  sha1: string;
  enabled: boolean;
  source: ModSource | null;
  projectId: string | null;
  /** The graph's root for this jar; the graph roots enabled platform mods only. */
  root: DepRoot | undefined;
  /** Required dependencies by project through the subtree (`depCounts`) — the cell's ⛓. */
  depTotal: number;
};

/** The mod the tour shows (A), the installed library row it requires on the same page (B). */
export type DepsTourTarget = {
  modSha1: string;
  librarySha1: string | null;
  hasOptional: boolean;
};

/**
 * A: enabled, a graph root whose dependencies are known, ⛓ > 0 (so Requires is non-empty). B: the
 * first top-level required dependency of A that is installed, is not A itself (a cycle node back to
 * the root sits in `required` too) and has an enabled row on this page. The first A with a B wins;
 * else the first A; else null.
 */
export function pickDepsTourTarget(page: readonly DepsTourRow[]): DepsTourTarget | null {
  let fallback: DepsTourTarget | null = null;
  for (const r of page) {
    const root = r.root;
    if (!r.enabled || !root || root.deps_unknown || r.depTotal <= 0) continue;
    const self = `${root.source}:${root.project_id}`;
    const library = root.required
      .filter((n) => n.installed && `${n.source}:${n.project_id}` !== self)
      .map((n) =>
        page.find(
          (p) =>
            p.enabled && p.sha1 !== r.sha1 && p.source === n.source && p.projectId === n.project_id,
        ),
      )
      .find((p) => p !== undefined);
    const target = {
      modSha1: r.sha1,
      librarySha1: library?.sha1 ?? null,
      hasOptional: root.optional.length > 0,
    };
    if (library) return target;
    fallback ??= target;
  }
  return fallback;
}

/** The tour's steps for a target: Optional and Required by only when their anchor will exist. */
export function depsTourSteps(t: DepsTourTarget): TourStep[] {
  const [cell, requires, optional, requiredBy] = DEPS_STEPS;
  return [
    cell,
    requires,
    ...(t.hasOptional ? [optional] : []),
    ...(t.librarySha1 !== null ? [requiredBy] : []),
  ];
}

/**
 * idle: the list is not on screen · armed: entered, waiting for a settled list and a free screen ·
 * held: the target is picked, the tour is mounted while no other surface owns the screen · spent:
 * this entry's attempt found nothing (spec §3.2, D6).
 */
export type DepsTourState =
  | { kind: 'idle' }
  | { kind: 'armed'; instanceId: string | null }
  | { kind: 'held'; instanceId: string | null; target: DepsTourTarget }
  | { kind: 'spent'; instanceId: string | null };

export const DEPS_TOUR_IDLE: DepsTourState = { kind: 'idle' };

export type DepsTourInput = {
  /** The Installed list of mods is the one on screen, in client mode. */
  onInstalled: boolean;
  instanceId: string | null;
  /** No other surface owns the screen and no contextual tour is running. */
  screenFree: boolean;
  /** This profile's list, its graph and its pre-flight have answered; nothing re-checks. */
  settled: boolean;
  /** The focus is in a text field — a search being typed. A card that took the focus now would
   *  turn the next Enter into "Next" (D6's reason), so the attempt is spent instead. */
  typing: boolean;
  /** Read only at the moment of the attempt. */
  pick: () => DepsTourTarget | null;
};

/** The next state; the SAME object when nothing changes, so the driving effect settles. */
export function nextDepsTourState(prev: DepsTourState, input: DepsTourInput): DepsTourState {
  if (!input.onInstalled) return DEPS_TOUR_IDLE;
  // An entry, or a profile switch while the list stays on screen: a new attempt.
  if (prev.kind === 'idle' || prev.instanceId !== input.instanceId) {
    return attempt({ kind: 'armed', instanceId: input.instanceId }, input);
  }
  if (prev.kind === 'armed') return attempt(prev, input);
  return prev;
}

function attempt(
  armed: Extract<DepsTourState, { kind: 'armed' }>,
  input: DepsTourInput,
): DepsTourState {
  if (!input.screenFree || !input.settled) return armed;
  if (input.typing) return { kind: 'spent', instanceId: armed.instanceId };
  const target = input.pick();
  return target
    ? { kind: 'held', instanceId: armed.instanceId, target }
    : { kind: 'spent', instanceId: armed.instanceId };
}

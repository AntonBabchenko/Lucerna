import { untrack } from 'svelte';
import { SvelteMap } from 'svelte/reactivity';
import {
  commands,
  type DepNameQuery,
  type DepNameResolved,
  type DepProjectKey,
  type PreflightReport,
  type ViolationKind,
} from '$lib/ipc/bindings';

/**
 * Human names for pre-flight dependency ids, held once for the whole app (spec 2026-09-28 §6.4).
 * Keyed by instance + (dependent sha1, dep id): two mods may declare the same bare mod-id and mean
 * different projects (audit A-F3), so an answer belongs to the mod that declared the id.
 * Module-level, like compat-scan.svelte.ts, so the answers outlive the Installed tab and any
 * surface can read them without a network round — nothing may sit between the user and Play.
 *
 * Only a caller that may wait (the Installed tab) calls `resolveDepNames`; everyone else reads
 * what is known and falls back to the raw id. An answer is a fact about an exact jar (its sha1),
 * so it is kept for the session: an updated mod is a new sha1 and so a new key. An unresolved
 * pair stays absent — the honest "unknown" — and is asked about again with the next report.
 */
export type DepNameEntry = { name: string; project: DepProjectKey | null };

/** The kinds whose dependency is named through the dependent's platform metadata. */
const NAMED: ReadonlySet<ViolationKind> = new Set<ViolationKind>([
  'missing_required',
  'required_disabled',
]);

const byInstance = new SvelteMap<string, SvelteMap<string, DepNameEntry>>();
// Pairs being asked about right now, by instance + pair. Every mod toggle re-runs the pre-flight,
// so a new report can arrive while the last one's names are still out: its pairs join that call.
const pending = new Map<string, Promise<void>>();
// Bumped by the test reset; a call that started before it writes nothing.
let epoch = 0;

export function depNameKey(dependentSha1: string, depId: string): string {
  return `${dependentSha1}\u0000${depId}`;
}

const pendingKey = (instanceId: string, key: string): string => `${instanceId}\u0000${key}`;

/** What is known about the dependency `depId` declared by `dependentSha1`, or null. Reactive. */
export function depNameEntry(
  instanceId: string | null,
  dependentSha1: string,
  depId: string,
): DepNameEntry | null {
  if (!instanceId) return null;
  return byInstance.get(instanceId)?.get(depNameKey(dependentSha1, depId)) ?? null;
}

/** The display name, or null — callers fall back to the raw dep id. Reactive. */
export function depNameOf(
  instanceId: string | null,
  dependentSha1: string,
  depId: string,
): string | null {
  return depNameEntry(instanceId, dependentSha1, depId)?.name ?? null;
}

/** The platform project the dependency resolved to, or null (unknown). Reactive. */
export function depProjectOf(
  instanceId: string | null,
  dependentSha1: string,
  depId: string,
): DepProjectKey | null {
  return depNameEntry(instanceId, dependentSha1, depId)?.project ?? null;
}

/**
 * Ask for every name `report` needs that is neither known nor already being asked about. Settles
 * once each of them has been answered (or found unresolvable); never rejects. Reads the store
 * untracked, so an `$effect` that calls it never re-runs on the answers it writes.
 */
export function resolveDepNames(instanceId: string, report: PreflightReport): Promise<void> {
  return untrack(() => {
    const known = byInstance.get(instanceId);
    const ask = new Map<string, DepNameQuery>();
    const waits = new Set<Promise<void>>();
    for (const v of report.violations) {
      if (!NAMED.has(v.kind)) continue;
      const key = depNameKey(v.dependent_sha1, v.dep_id);
      if (known?.has(key) || ask.has(key)) continue;
      const running = pending.get(pendingKey(instanceId, key));
      if (running) waits.add(running);
      else ask.set(key, { dependent_sha1: v.dependent_sha1, dep_id: v.dep_id });
    }
    if (ask.size > 0) waits.add(askPlatform(instanceId, ask));
    return Promise.all(waits).then(() => undefined);
  });
}

function askPlatform(instanceId: string, ask: Map<string, DepNameQuery>): Promise<void> {
  const keys = [...ask.keys()].map((key) => pendingKey(instanceId, key));
  const run = answer(instanceId, [...ask.values()], epoch);
  for (const k of keys) pending.set(k, run);
  // `answer` never rejects, so neither does this cleanup. It runs before any caller resumes.
  void run.finally(() => {
    for (const k of keys) if (pending.get(k) === run) pending.delete(k);
  });
  return run;
}

async function answer(instanceId: string, queries: DepNameQuery[], at: number): Promise<void> {
  try {
    const res = await commands.modsResolveDepNames(instanceId, queries);
    if (res.status !== 'ok' || at !== epoch) return;
    remember(instanceId, res.data);
  } catch {
    // Enrichment, not a recovery path: the restrictive answer is no name — the raw id is shown,
    // which honestly describes what we know — and no failed operation goes unchecked. It cannot
    // tell "nothing resolved" from "the call never landed" and need not: both mean no name, and
    // the pair is asked about again with the next report. This only keeps a transport-level
    // rejection from escaping into the caller's `$effect`.
  }
}

function remember(instanceId: string, answers: DepNameResolved[]): void {
  // A blank name would render as nothing at all; the raw id says more.
  const named = answers.filter((r) => r.name.trim() !== '');
  if (named.length === 0) return;
  let map = byInstance.get(instanceId);
  if (!map) {
    map = new SvelteMap<string, DepNameEntry>();
    byInstance.set(instanceId, map);
  }
  for (const r of named) {
    map.set(depNameKey(r.dependent_sha1, r.dep_id), { name: r.name, project: r.project });
  }
}

/** Test-only: the store is module-level on purpose (it must outlive the Installed tab). */
export function __resetDepNamesForTests(): void {
  epoch++;
  byInstance.clear();
  pending.clear();
}

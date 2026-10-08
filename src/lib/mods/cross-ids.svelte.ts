// Each installed jar's id on the other platform (spec 2026-10-08 cross-source identity; follow-up
// 2026-10-08 aliases-everywhere, D3/D4). Three things per profile, shared by every view:
//
// - the learning pass, one at a time: a view that mounts while a pass runs joins it instead of
//   starting a second network pass, and a call made meanwhile runs it once more after;
// - a generation the page moves when the backend says a pass learned an id
//   (`mods-cross-ids-learned`) — views read it as state, so none of them subscribes to IPC;
// - the alias map, alias key → own key (`source:project_id`), read through `mods_cross_aliases`.
import { untrack } from 'svelte';
import { SvelteMap } from 'svelte/reactivity';
import { commands, type InstalledMod, type ModSource } from '$lib/ipc/bindings';

const generations = new SvelteMap<string, number>();
const aliasMaps = new SvelteMap<string, ReadonlyMap<string, string>>();
const running = new Map<string, Promise<void>>();
const again = new Set<string>();
const aliasReads = new Map<string, number>();
const NONE: ReadonlyMap<string, string> = new Map();

/** How many times a pass has learned something for this profile this session. */
export function crossIdsGeneration(id: string): number {
  return generations.get(id) ?? 0;
}

/** A pass learned an id for this profile: every view reading its generation re-reads. */
export function bumpCrossIds(id: string): void {
  generations.set(id, untrack(() => crossIdsGeneration(id)) + 1);
}

/** The profile's alias map as last read; empty until read. */
export function aliasesFor(id: string): ReadonlyMap<string, string> {
  return aliasMaps.get(id) ?? NONE;
}

/** Ask the backend to learn the profile's ids on the other platform. Nothing to ask is no
 *  request there. Best-effort, as the enrich pass: a failure is logged, never thrown. */
export async function learnCrossIds(id: string): Promise<void> {
  const inFlight = running.get(id);
  if (inFlight) {
    again.add(id);
    return inFlight;
  }
  const pass = (async () => {
    try {
      const r = await commands.modsLearnCrossIds(id);
      if (r.status === 'error') console.warn('[cross-ids] learning failed:', r.error);
    } catch (e) {
      console.warn('[cross-ids] learning failed:', e);
    }
  })();
  running.set(id, pass);
  try {
    await pass;
  } finally {
    running.delete(id);
  }
  if (again.delete(id)) await learnCrossIds(id);
}

/** Read the profile's alias map. A failure — the backend could not read its sidecar, or a missing
 *  command (an older test mock) throws — keeps the previous map, empty at first, so every lookup
 *  behaves as it did before aliases existed. Reads overlap (a list load and a learned id); the
 *  latest one asked for wins, never an older answer that lands last. */
export async function loadAliases(id: string): Promise<void> {
  const ticket = (aliasReads.get(id) ?? 0) + 1;
  aliasReads.set(id, ticket);
  try {
    const r = await commands.modsCrossAliases(id);
    if (aliasReads.get(id) !== ticket) return;
    if (r.status === 'error') {
      console.warn('[cross-ids] alias map unreadable:', r.error);
      return;
    }
    aliasMaps.set(
      id,
      new Map(
        r.data.map((a) => [
          `${a.alias_source}:${a.alias_project_id}`,
          `${a.own_source}:${a.own_project_id}`,
        ]),
      ),
    );
  } catch (e) {
    console.warn('[cross-ids] alias map unreadable:', e);
  }
}

const keyOf = (m: InstalledMod): string | null =>
  m.source !== null && m.project_id !== null ? `${m.source}:${m.project_id}` : null;

/** The installed row a project is: the row with its own key, else the row it is an alias of.
 *  Never a name — a switch replaces the jar this returns. */
export function findInstalled<T extends { installed: InstalledMod }>(
  rows: readonly T[],
  aliases: ReadonlyMap<string, string>,
  source: ModSource,
  projectId: string,
): T | null {
  const key = `${source}:${projectId}`;
  const own = rows.find((r) => keyOf(r.installed) === key);
  if (own) return own;
  const alias = aliases.get(key);
  return alias ? (rows.find((r) => keyOf(r.installed) === alias) ?? null) : null;
}

/** Module state outlives a test; a test that relies on it starts from nothing. */
export function resetCrossIdsForTests(): void {
  generations.clear();
  aliasMaps.clear();
  running.clear();
  again.clear();
  aliasReads.clear();
}

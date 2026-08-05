import type { ModSource, VtCategory, VtPack } from '$lib/ipc/bindings';

/**
 * A pack's identity, both in the tick set and in the registry row Lucerna
 * writes. The category is part of it because the build request is keyed by
 * category — a pack name alone cannot rebuild the request when an update
 * comes.
 */
export function packId(category: string, pack: VtPack): string {
  return `${category}/${pack.name}`;
}

/**
 * The ticked packs grouped by category — the exact shape the build request
 * takes. A category with nothing ticked is omitted rather than sent empty.
 */
export function toSelection(
  categories: VtCategory[],
  ticked: Set<string>,
): [string, string[]][] {
  const out: [string, string[]][] = [];
  for (const cat of categories) {
    const names = cat.packs.filter((p) => ticked.has(packId(cat.category, p))).map((p) => p.name);
    if (names.length > 0) out.push([cat.category, names]);
  }
  return out;
}

/**
 * Display names of ticked packs that conflict with `pack`.
 *
 * Incompatibility is declared on one side only — `graves` names
 * `armor statues`, but not the other way round — so this checks BOTH
 * directions. A pack that declares nothing can still be named by somebody.
 */
export function conflictsFor(
  pack: VtPack,
  categories: VtCategory[],
  ticked: Set<string>,
): string[] {
  const declares = (a: VtPack, b: VtPack) =>
    a.incompatible.some((n) => n.toLowerCase() === b.name.toLowerCase());
  return categories
    .flatMap((c) => c.packs.map((p) => ({ cat: c.category, p })))
    .filter((e) => e.p.name !== pack.name)
    .filter((e) => ticked.has(packId(e.cat, e.p)))
    .filter((e) => declares(pack, e.p) || declares(e.p, pack))
    .map((e) => e.p.display);
}

/** The subset of a library/sidecar row this module needs. */
type VtRowish = {
  source: ModSource | null;
  project_id: string | null;
  version_id: string | null;
};

/**
 * `packId → installed version` for the Vanilla Tweaks rows among `rows`.
 * Drives both the pre-ticked checkboxes and the "already added" marker, so the
 * builder never stores a selection of its own — the registry is the record.
 */
export function installedVtPacks(rows: VtRowish[]): Map<string, string> {
  const out = new Map<string, string>();
  for (const r of rows) {
    if (r.source !== 'vanilla_tweaks') continue;
    if (!r.project_id) continue;
    out.set(r.project_id, r.version_id ?? '');
  }
  return out;
}

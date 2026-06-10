// Given all of a mod's versions (across every loader) plus the Minecraft
// version manifest order, work out the latest Minecraft version the mod
// supports for each loader it actually ships. Used to turn the generic
// "no compatible version" install error into an actionable message
// ("Forge tops out at 1.19.2, Fabric goes to 1.21").

import type { LoaderKind, ModVersion } from '$lib/ipc/bindings';

export interface LoaderLatest {
  loader: LoaderKind;
  mcVersion: string;
}

// Stable display order. `vanilla` is intentionally absent — a mod jar is never
// "for vanilla", and we skip that loader entry below.
const LOADER_ORDER: readonly LoaderKind[] = ['fabric', 'forge', 'neoforge', 'quilt'];

/**
 * Latest supported MC version per loader, in {@link LOADER_ORDER}. Loaders the
 * mod has no version for are omitted. "Latest" is decided by the manifest
 * order (index 0 = newest); MC versions absent from the manifest (snapshots,
 * unknowns) rank last and are only chosen when nothing better exists.
 */
export function latestSupportedPerLoader(
  allVersions: ModVersion[],
  manifestOrder: string[],
): LoaderLatest[] {
  const rankOf = new Map(manifestOrder.map((id, i) => [id, i]));
  // Per loader: the best (newest) MC version seen so far and its manifest rank.
  const best = new Map<LoaderKind, { mc: string; rank: number }>();

  for (const version of allVersions) {
    for (const loader of version.loaders) {
      if (loader === 'vanilla') continue;
      for (const mc of version.mc_versions) {
        const rank = rankOf.has(mc) ? (rankOf.get(mc) as number) : Number.POSITIVE_INFINITY;
        const current = best.get(loader);
        if (!current || rank < current.rank) best.set(loader, { mc, rank });
      }
    }
  }

  return LOADER_ORDER.filter((loader) => best.has(loader)).map((loader) => ({
    loader,
    mcVersion: (best.get(loader) as { mc: string }).mc,
  }));
}

/**
 * Render a per-loader latest list as a brand-neutral, locale-independent string:
 * `"Fabric 1.21 · Forge 1.19.2"`. Empty input → `''`. The surrounding sentence
 * (which locale, what label) is i18n at the call site; only this inner list is
 * built in code so it reads identically in every language.
 */
export function formatLoaderLatestList(
  perLoader: LoaderLatest[],
  displayLoader: (k: LoaderKind) => string,
): string {
  return perLoader.map((p) => `${displayLoader(p.loader)} ${p.mcVersion}`).join(' · ');
}

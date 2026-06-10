// Classify each "unresolvable" required dependency (one the backend could not
// satisfy for the instance's MC + loader) into a display-agnostic shape the
// DependencyDialog can render with precise i18n — mirroring the primary mod's
// reportNoCompatibleVersion split, one level down. No i18n and no IPC here:
// fetchers are injected (deps.rs-style) so this unit-tests in isolation.

import type { DepProjectRef, LoaderKind, ModVersion } from '$lib/ipc/bindings';
import {
  formatLoaderLatestList,
  latestSupportedPerLoader,
} from '$lib/mods/latest-supported-version';

/**
 * A required dependency with no version for the instance's MC + loader.
 * `list` is the brand-neutral "Fabric 1.21 · Forge 1.19.2" string; the
 * `noVersions` variant has no list because nothing usable was found.
 */
export type UnresolvableDetail =
  | { kind: 'noMc'; name: string; mcVersion: string; list: string }
  | { kind: 'wrongLoader'; name: string; loader: string; list: string }
  | { kind: 'noVersions'; name: string };

export interface UnresolvableDeps {
  /** All of the dep's versions across every loader/MC, or null if the lookup failed. */
  fetchVersions: (ref: DepProjectRef) => Promise<ModVersion[] | null>;
  /** Human-readable project name (falls back to the raw id at the call site). */
  fetchProjectName: (ref: DepProjectRef) => Promise<string>;
  /** MC version manifest, newest-first (index 0 = newest). */
  manifestOrder: string[];
  /** Brand-neutral loader display name. */
  displayLoader: (k: LoaderKind) => string;
  /** The active instance's loader + MC version (both known before deps resolve). */
  currentLoader: LoaderKind;
  mcVersion: string;
}

export async function enrichUnresolvable(
  refs: DepProjectRef[],
  deps: UnresolvableDeps,
): Promise<UnresolvableDetail[]> {
  return Promise.all(
    refs.map(async (ref): Promise<UnresolvableDetail> => {
      const name = await deps.fetchProjectName(ref);
      const all = await deps.fetchVersions(ref);
      if (!all) return { kind: 'noVersions', name };
      const perLoader = latestSupportedPerLoader(all, deps.manifestOrder);
      if (perLoader.length === 0) return { kind: 'noVersions', name };
      const list = formatLoaderLatestList(perLoader, deps.displayLoader);
      if (perLoader.some((p) => p.loader === deps.currentLoader)) {
        return { kind: 'noMc', name, mcVersion: deps.mcVersion, list };
      }
      return { kind: 'wrongLoader', name, loader: deps.displayLoader(deps.currentLoader), list };
    }),
  );
}

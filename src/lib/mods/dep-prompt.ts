// Dependency-prompt decision logic extracted from ModBrowseView's startInstall.
//
// `decideModInstall` takes the resolved InstallPlan and a small DI context
// (fetchProjectName, fetchVersions, displayLoader, etc.) and returns either
// { kind: 'install' } (fast path — nothing to show, install directly) or
// { kind: 'prompt', prompt } (open the DependencyDialog with the enriched data).
//
// All network calls go through the injected context so this module is fully
// unit-testable without IPC.

import type {
  DepProjectRef,
  InstallPlan,
  LoaderKind,
  ModSource,
  ModVersion,
} from '$lib/ipc/bindings';
import { enrichUnresolvable, type UnresolvableDetail } from '$lib/mods/unresolvable-detail';

// --------------------------------------------------------------------------
// Shared types (also consumed by install-summary.ts and DependencyDialog)
// --------------------------------------------------------------------------

/** An enriched dependency item: version + human-readable project name. */
export type DepItem = {
  version: ModVersion;
  projectName: string;
  projectSource: ModSource;
};

/** An optional dependency with its own transitive required sub-deps. */
export type OptionalItem = DepItem & { requires: DepItem[] };

/** The full enriched data set that the DependencyDialog renders. */
export type DepPromptData = {
  primary: ModVersion;
  primaryProjectName: string;
  required: DepItem[];
  optional: OptionalItem[];
  incompatible: string[];
  unresolvable: UnresolvableDetail[];
  /** Loader project names (NeoForge, Fabric, …) — informational only. */
  loaderRequirements: string[];
  /** Non-null when the picked version's loaders don't include the active loader. */
  loaderMismatch: { instanceLoader: string; modLoaders: LoaderKind[] } | null;
};

/** The return type of {@link decideModInstall}. */
export type ModInstallDecision =
  | { kind: 'install'; primaryProjectName: string }
  | { kind: 'prompt'; prompt: DepPromptData };

// --------------------------------------------------------------------------
// Injection context
// --------------------------------------------------------------------------

export interface DecideModInstallCtx {
  loader: LoaderKind;
  mcVersion: string;
  /** MC version manifest ids, newest-first (index 0 = newest). */
  manifestOrder: string[];
  displayLoader: (l: LoaderKind) => string;
  /** Returns null on lookup failure (network / deleted project). */
  fetchProjectName: (source: ModSource, id: string) => Promise<string | null>;
  /** Returns null on lookup failure. */
  fetchVersions: (source: ModSource, id: string) => Promise<ModVersion[] | null>;
}

// --------------------------------------------------------------------------
// Decision function
// --------------------------------------------------------------------------

/**
 * Enrich the raw `InstallPlan` with human-readable names and decide whether
 * to install directly (fast path) or open the DependencyDialog.
 *
 * The logic is extracted verbatim from ModBrowseView's `startInstall`.
 */
export async function decideModInstall(
  p: InstallPlan,
  primary: ModVersion,
  ctx: DecideModInstallCtx,
): Promise<ModInstallDecision> {
  // Helper: resolve source + id for a DepProjectRef.
  function refSourceId(r: DepProjectRef): { source: ModSource; id: string } {
    if ('project_id' in r) {
      return { source: 'modrinth', id: r.project_id };
    }
    return { source: 'curseforge', id: String(r.mod_id) };
  }

  // Enrich a ModVersion to a DepItem.
  const enrichDep = async (v: ModVersion): Promise<DepItem> => {
    const name = await ctx.fetchProjectName(v.source, v.project_id);
    return {
      version: v,
      projectName: name ?? v.name,
      projectSource: v.source,
    };
  };

  // Enrich a DepProjectRef to a human-readable project name.
  const enrichRefName = async (r: DepProjectRef): Promise<string> => {
    const { source, id } = refSourceId(r);
    const name = await ctx.fetchProjectName(source, id);
    return name ?? id;
  };

  // `InstallPlan.required` / `OptionalDep.requires` are now `PlannedDep[]`
  // ({ version, selection_reason }). We unwrap `.version` here — the
  // selection_reason is data-ready but intentionally not rendered this round.
  const requiredEnriched = await Promise.all(p.required.map((d) => enrichDep(d.version)));

  const optionalEnriched: OptionalItem[] = await Promise.all(
    p.optional.map(async (o): Promise<OptionalItem> => {
      const top = await enrichDep(o.version);
      const subReqs = await Promise.all(o.requires.map((d) => enrichDep(d.version)));
      return { ...top, requires: subReqs };
    }),
  );

  const incompatibleNames = await Promise.all(p.incompatible.map(enrichRefName));

  const unresolvableDetails = await enrichUnresolvable(p.unresolvable, {
    fetchVersions: async (ref) => {
      const { source, id } = refSourceId(ref);
      return ctx.fetchVersions(source, id);
    },
    fetchProjectName: enrichRefName,
    manifestOrder: ctx.manifestOrder,
    displayLoader: ctx.displayLoader,
    currentLoader: ctx.loader,
    mcVersion: ctx.mcVersion,
  });

  const loaderRequirements = await Promise.all(p.loader_requirements.map(enrichRefName));

  const primaryProjectName =
    (await ctx.fetchProjectName(primary.source, primary.project_id)) ?? primary.name;

  const loaderMismatch =
    primary.loaders.length > 0 && !primary.loaders.includes(ctx.loader)
      ? { instanceLoader: ctx.loader, modLoaders: primary.loaders }
      : null;

  // Fast path: nothing to show → install directly.
  if (
    requiredEnriched.length === 0 &&
    optionalEnriched.length === 0 &&
    p.incompatible.length === 0 &&
    p.unresolvable.length === 0 &&
    loaderRequirements.length === 0 &&
    loaderMismatch === null
  ) {
    return { kind: 'install', primaryProjectName };
  }

  return {
    kind: 'prompt',
    prompt: {
      primary,
      primaryProjectName,
      required: requiredEnriched,
      optional: optionalEnriched,
      incompatible: incompatibleNames,
      unresolvable: unresolvableDetails,
      loaderRequirements,
      loaderMismatch,
    },
  };
}

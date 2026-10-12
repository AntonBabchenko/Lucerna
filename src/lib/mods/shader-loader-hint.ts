// Version-aware shader-loader guidance for the Add-ons → Shaders tab.
//
// Shaders need a shader loader, and which one works depends on the mod loader
// AND the Minecraft version, with holes no range can express (Iris has NeoForge
// builds for 1.21.8 and 1.21.10 but not 1.21.9). So this module only says WHAT
// to check; `shader-loader-availability.ts` asks Modrinth and the loader lists,
// and `shaderBannerState` turns the answers into one banner state. Pure — no
// IPC here. Spec: docs/superpowers/specs/2026-09-29-shader-loader-version-aware-design.md.

import type { InstalledMod } from '$lib/ipc/bindings';

export type Loader = 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;
export type ModLoader = 'fabric' | 'quilt' | 'forge' | 'neoforge';
/** The mod loaders a vanilla profile can be pointed at, in display order. */
export type InstallTarget = 'fabric' | 'neoforge' | 'forge';

export type ShaderLoaderId = 'iris' | 'oculus' | 'optifine';
/** The shader loaders published on Modrinth, so checkable live. */
export type ModrinthShaderLoader = 'iris' | 'oculus';

// Canonical Modrinth base62 project ids — the same ids the mod detail modal
// keys installed-state and project-cache lookups on.
export const IRIS_MODRINTH_PROJECT_ID = 'YL57xq9U';
export const OCULUS_MODRINTH_PROJECT_ID = 'GchcoXML';

export interface ShaderCheck {
  shader: ModrinthShaderLoader;
  loader: ModLoader;
}

export interface CheckPlan {
  shader: ShaderCheck[];
  install: InstallTarget[];
}

export type ShaderAvailability = 'available' | 'no_build' | 'unknown';
export type InstallAvailability = 'installable' | 'not_installable' | 'unknown';

/** Answers keyed by `shaderCheckKey` / install target. A missing entry is pending. */
export interface Outcomes {
  shader: Readonly<Record<string, ShaderAvailability>>;
  install: Readonly<Partial<Record<InstallTarget, InstallAvailability>>>;
}

export const shaderCheckKey = (c: ShaderCheck): string => `${c.shader}:${c.loader}`;

const NO_CHECKS: CheckPlan = { shader: [], install: [] };

/** What to ask for a profile on `loader`. Quilt also asks Iris×fabric: it tells
 *  "Iris has a Fabric version not marked for Quilt" apart from "none exists". A
 *  vanilla profile asks every route to shaders plus whether Lucerna can install
 *  each mod loader for this Minecraft version (the loader picker's own source). */
export function checkPlan(loader: Loader): CheckPlan {
  switch (loader) {
    case 'fabric':
      return { shader: [{ shader: 'iris', loader: 'fabric' }], install: [] };
    case 'quilt':
      return {
        shader: [
          { shader: 'iris', loader: 'quilt' },
          { shader: 'iris', loader: 'fabric' },
        ],
        install: [],
      };
    case 'neoforge':
      return {
        shader: [
          { shader: 'iris', loader: 'neoforge' },
          { shader: 'oculus', loader: 'neoforge' },
        ],
        install: [],
      };
    case 'forge':
      return { shader: [{ shader: 'oculus', loader: 'forge' }], install: [] };
    case 'vanilla':
      return {
        shader: [
          { shader: 'iris', loader: 'fabric' },
          { shader: 'iris', loader: 'neoforge' },
          { shader: 'oculus', loader: 'forge' },
        ],
        install: ['fabric', 'neoforge', 'forge'],
      };
    default:
      return NO_CHECKS;
  }
}

/** Shader loaders whose presence hides the banner. Deliberately not filtered by
 *  Minecraft version: a jar built for another version is the installed-mods
 *  compatibility check's finding, not this hint's. */
export function installedDetectionSet(loader: Loader): ShaderLoaderId[] {
  switch (loader) {
    case 'fabric':
    case 'quilt':
      return ['iris'];
    case 'neoforge':
      return ['iris', 'oculus'];
    case 'forge':
      return ['oculus', 'optifine'];
    default:
      return [];
  }
}

export type OptifineForgeBuild = 'release' | 'preview';

// Minecraft versions with an OptiFine build that lists a Forge version, from
// optifine.net/downloads, checked 2026-09-29 and again 2026-10-12 (unchanged).
// Keys are Mojang ids: the site's "1.8.0" / "1.9.0" are Mojang's 1.8 / 1.9.
// Release builds stay on the site for years (1.7.2's are from 2018), so a
// 'release' entry can only lag. Previews are pruned and promoted, so 'preview'
// entries can go stale; their copy says "as far as Lucerna knows". Refresh:
// re-parse the page (rows downloadLineMain/More/Preview, column colForge).
const OPTIFINE_FORGE_BUILDS: ReadonlyMap<string, OptifineForgeBuild> = new Map([
  ...[
    '1.7.2',
    '1.7.10',
    '1.8',
    '1.8.8',
    '1.8.9',
    '1.9',
    '1.9.2',
    '1.9.4',
    '1.10',
    '1.10.2',
    '1.11',
    '1.11.2',
    '1.12',
    '1.12.1',
    '1.12.2',
    '1.14.4',
    '1.15.2',
    '1.16.1',
    '1.16.2',
    '1.16.3',
    '1.16.4',
    '1.16.5',
    '1.17.1',
    '1.18',
    '1.18.1',
    '1.18.2',
    '1.19',
    '1.19.1',
    '1.19.2',
    '1.19.3',
    '1.19.4',
    '1.20.1',
    '1.20.4',
    '1.21.1',
    '1.21.3',
    '1.21.4',
    '1.21.11',
  ].map((mc) => [mc, 'release'] as const),
  ...['1.20', '1.20.6', '1.21.8', '1.21.10'].map((mc) => [mc, 'preview'] as const),
]);

/** Whether OptiFine has a Forge-compatible build for `mc` (exact Mojang id). */
export function optifineForgeBuild(mc: string): OptifineForgeBuild | null {
  return OPTIFINE_FORGE_BUILDS.get(mc) ?? null;
}

export interface VanillaLine {
  loader: InstallTarget;
  /** In display order; 'optifine' last when present. */
  shaders: ShaderLoaderId[];
  optifine: OptifineForgeBuild | null;
}

export type ShaderBannerState =
  | { kind: 'hidden' }
  | { kind: 'pending' }
  /** N — no profile selected. */
  | { kind: 'no_instance' }
  /** G — a profile without a mod loader or Minecraft version recorded. */
  | { kind: 'generic' }
  /** M1 */
  | {
      kind: 'options';
      buttons: ShaderLoaderId[];
      optifine: OptifineForgeBuild | null;
      unchecked: ModrinthShaderLoader[];
    }
  /** M2 */
  | { kind: 'unchecked'; unchecked: ModrinthShaderLoader[] }
  /** Q */
  | { kind: 'quilt_fabric_only' }
  /** M3 */
  | { kind: 'none_found'; variant: 'iris' | 'iris_oculus' | 'forge' }
  /** V1 */
  | { kind: 'vanilla_options'; lines: VanillaLine[]; unchecked: InstallTarget[] }
  /** V2 */
  | { kind: 'vanilla_unchecked' }
  /** V3 */
  | { kind: 'vanilla_none_found' };

export interface BannerInput {
  instanceSelected: boolean;
  loader: Loader;
  mcVersion: string | null;
  /** `null` = not read yet for this profile. */
  installed: InstalledMod[] | null;
  /** `null` = no answer yet for this profile. */
  outcomes: Outcomes | null;
}

const HIDDEN: ShaderBannerState = { kind: 'hidden' };
const PENDING: ShaderBannerState = { kind: 'pending' };
const INSTALL_TARGETS: readonly InstallTarget[] = ['fabric', 'neoforge', 'forge'];

/** The one banner state for a profile. Evaluation order: no profile → N; no
 *  loader/MC → G; modded and the installed list unread or holding an applicable
 *  loader → hidden; any answer missing → pending; then the modded or vanilla
 *  table (spec §4.3). */
export function shaderBannerState(input: BannerInput): ShaderBannerState {
  const { loader, mcVersion, installed, outcomes } = input;
  if (!input.instanceSelected) return { kind: 'no_instance' };
  if (loader === null || !mcVersion) return { kind: 'generic' };
  if (loader !== 'vanilla') {
    if (installed === null) return HIDDEN;
    if (detectInstalledShaderLoaders(installed, installedDetectionSet(loader)).length > 0) {
      return HIDDEN;
    }
  }
  if (outcomes === null) return PENDING;
  const plan = checkPlan(loader);
  const answer = (c: ShaderCheck) => outcomes.shader[shaderCheckKey(c)];
  if (plan.shader.some((c) => answer(c) === undefined)) return PENDING;
  if (plan.install.some((target) => outcomes.install[target] === undefined)) return PENDING;
  return loader === 'vanilla'
    ? vanillaState(plan, mcVersion, outcomes)
    : moddedState(loader, plan, mcVersion, outcomes);
}

function moddedState(
  loader: ModLoader,
  plan: CheckPlan,
  mc: string,
  outcomes: Outcomes,
): ShaderBannerState {
  const answer = (c: ShaderCheck) => outcomes.shader[shaderCheckKey(c)];
  const optifine = loader === 'forge' ? optifineForgeBuild(mc) : null;
  const confirmed: ShaderLoaderId[] = plan.shader
    .filter((c) => c.loader === loader && answer(c) === 'available')
    .map((c) => c.shader);
  const buttons: ShaderLoaderId[] = optifine ? [...confirmed, 'optifine'] : confirmed;
  const unchecked = unique(
    plan.shader.filter((c) => answer(c) === 'unknown').map((c) => c.shader),
  ).filter((s) => !buttons.includes(s));
  if (buttons.length > 0) return { kind: 'options', buttons, optifine, unchecked };
  if (unchecked.length > 0) return { kind: 'unchecked', unchecked };
  if (loader === 'quilt' && answer({ shader: 'iris', loader: 'fabric' }) === 'available') {
    return { kind: 'quilt_fabric_only' };
  }
  const variant = loader === 'neoforge' ? 'iris_oculus' : loader === 'forge' ? 'forge' : 'iris';
  return { kind: 'none_found', variant };
}

function vanillaState(plan: CheckPlan, mc: string, outcomes: Outcomes): ShaderBannerState {
  const answer = (c: ShaderCheck) => outcomes.shader[shaderCheckKey(c)];
  const lines: VanillaLine[] = [];
  const unchecked: InstallTarget[] = [];
  for (const target of INSTALL_TARGETS) {
    const checks = plan.shader.filter((c) => c.loader === target);
    const available: ShaderLoaderId[] = checks
      .filter((c) => answer(c) === 'available')
      .map((c) => c.shader);
    const optifine = target === 'forge' ? optifineForgeBuild(mc) : null;
    const shaders: ShaderLoaderId[] = optifine ? [...available, 'optifine'] : available;
    const install = outcomes.install[target];
    const shaderUnknown = checks.some((c) => answer(c) === 'unknown');
    if (shaders.length > 0 && install === 'installable') {
      lines.push({ loader: target, shaders, optifine });
    } else if (install !== 'not_installable' && (shaders.length > 0 || shaderUnknown)) {
      unchecked.push(target);
    }
  }
  if (lines.length > 0) return { kind: 'vanilla_options', lines, unchecked };
  if (unchecked.length > 0) return { kind: 'vanilla_unchecked' };
  return { kind: 'vanilla_none_found' };
}

function unique<T>(items: T[]): T[] {
  return [...new Set(items)];
}

// Per-source canonical project ids for the loaders published to a platform.
// Detection matches by id (NOT filename) so the many addon/compat mods whose
// names start with "iris"/"oculus" (e.g. "Irislowka", "Iris/Oculus Shader
// Folder") — which share the jar prefix but are NOT the loader — cannot
// false-positively hide the hint. CurseForge ids are internal (detection only);
// the Modrinth ids above are also used by the component for deep-linking.
const IRIS_CURSEFORGE_PROJECT_ID = '455508';
const OCULUS_CURSEFORGE_PROJECT_ID = '581495';

const PROJECT_IDS: Record<ShaderLoaderId, { modrinth?: string; curseforge?: string }> = {
  iris: { modrinth: IRIS_MODRINTH_PROJECT_ID, curseforge: IRIS_CURSEFORGE_PROJECT_ID },
  oculus: { modrinth: OCULUS_MODRINTH_PROJECT_ID, curseforge: OCULUS_CURSEFORGE_PROJECT_ID },
  optifine: {},
};

// OptiFine has no Modrinth/CurseForge project (site-only download), so a
// filename heuristic is its only signal. Anchored at the start; "OptiFine" is a
// distinctive prefix (real jars: "OptiFine_1.20.1_HD_U_I6.jar", incl. the "_MOD"
// Forge variant).
const OPTIFINE_FILENAME = /^optifine[-_.]/i;

function matchesShaderLoader(id: ShaderLoaderId, m: InstalledMod): boolean {
  const ids = PROJECT_IDS[id];
  if (m.project_id) {
    if (m.source === 'modrinth' && m.project_id === ids.modrinth) return true;
    if (m.source === 'curseforge' && m.project_id === ids.curseforge) return true;
  }
  if (id === 'optifine') {
    const base = m.filename.split(/[\\/]/).pop() ?? m.filename;
    return OPTIFINE_FILENAME.test(base);
  }
  return false;
}

/** Subset of `applicable` already installed on the instance (intersected with
 *  `applicable`, so an installed-but-incompatible loader does not count). The
 *  hint is hidden when this is non-empty. A disabled jar still counts — the
 *  loader is present on disk. */
export function detectInstalledShaderLoaders(
  installed: InstalledMod[],
  applicable: ShaderLoaderId[],
): ShaderLoaderId[] {
  return applicable.filter((id) => installed.some((m) => matchesShaderLoader(id, m)));
}

// Loader-aware shader-loader guidance for the Add-ons → Shaders tab.
//
// Shaders need a shader loader to run, and which loader works depends on the
// instance's mod loader (verified against the live Modrinth API 2026-06-14):
//   - Iris    (YL57xq9U): fabric, quilt, neoforge
//   - Oculus  (GchcoXML): forge, neoforge
//   - OptiFine          : vanilla (standalone) / Forge legacy (as a mod)
// We offer EVERY loader that actually works on the instance's loader rather
// than narrowing to one, so the user sees all valid paths.

import type { InstalledMod } from '$lib/ipc/bindings';

type Loader = 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;

export type ShaderLoaderId = 'iris' | 'oculus' | 'optifine';

// Canonical Modrinth base62 project ids — the same ids the mod detail modal
// keys installed-state and project-cache lookups on.
export const IRIS_MODRINTH_PROJECT_ID = 'YL57xq9U';
export const OCULUS_MODRINTH_PROJECT_ID = 'GchcoXML';

/** Shader loaders that actually run on the given instance loader, in display
 *  order. `null` (no instance selected) returns the full set — we cannot know
 *  which loader the user will pick. */
export function shaderLoaderOptions(loader: Loader): ShaderLoaderId[] {
  switch (loader) {
    case 'fabric':
    case 'quilt':
      return ['iris'];
    case 'forge':
      return ['oculus', 'optifine'];
    case 'neoforge':
      return ['iris', 'oculus'];
    case 'vanilla':
      return ['optifine'];
    default:
      return ['iris', 'oculus', 'optifine'];
  }
}

// Filename heuristics — source-agnostic, so they catch CurseForge and manual
// jars too. Anchored at the start for iris/oculus so a jar that merely contains
// the substring mid-name does not false-positive. OptiFine's name is
// distinctive enough to match anywhere (e.g. "OptiFine_1.20.1_HD_U_I6.jar",
// incl. the "_MOD" Forge variant).
const FILENAME_PATTERNS: Record<ShaderLoaderId, RegExp> = {
  iris: /^iris[-_.]/i,
  oculus: /^oculus[-_.]/i,
  optifine: /optifine/i,
};

const MODRINTH_IDS: Partial<Record<ShaderLoaderId, string>> = {
  iris: IRIS_MODRINTH_PROJECT_ID,
  oculus: OCULUS_MODRINTH_PROJECT_ID,
};

function matchesShaderLoader(id: ShaderLoaderId, m: InstalledMod): boolean {
  const modrinthId = MODRINTH_IDS[id];
  if (modrinthId && m.source === 'modrinth' && m.project_id === modrinthId) return true;
  const base = m.filename.split(/[\\/]/).pop() ?? m.filename;
  return FILENAME_PATTERNS[id].test(base);
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

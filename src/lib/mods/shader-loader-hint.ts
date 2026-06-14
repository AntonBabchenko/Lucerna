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

import type { LoaderKind, ServerCore } from '$lib/ipc/bindings';

// Backend serialises ServerCore as snake_case. Brand-canonical capitalisation
// for UI display, mirroring loader-display.ts (which stays client-only —
// server surfaces display cores through THIS map).
const DISPLAY: Record<ServerCore, string> = {
  vanilla: 'Vanilla',
  fabric: 'Fabric',
  quilt: 'Quilt',
  forge: 'Forge',
  neoforge: 'NeoForge',
  paper: 'Paper',
  purpur: 'Purpur',
};

export function displayCore(c: ServerCore): string {
  return DISPLAY[c];
}

/** The equivalent client loader; null for plugin cores (a Paper server pairs with a vanilla client). */
export function coreToLoaderKind(c: ServerCore): LoaderKind | null {
  return c === 'paper' || c === 'purpur' ? null : c;
}

export function pluginCapable(c: ServerCore): boolean {
  return c === 'paper' || c === 'purpur';
}

export function modCapable(c: ServerCore): boolean {
  return c === 'fabric' || c === 'quilt' || c === 'forge' || c === 'neoforge';
}

/** Cores the switch flow may offer as targets from the given core (mirrors the Rust core_switch_allowed matrix). */
export function switchTargets(c: ServerCore): ServerCore[] {
  if (c === 'vanilla') return ['paper', 'purpur'];
  if (c === 'paper') return ['purpur'];
  if (c === 'purpur') return ['paper'];
  return [];
}

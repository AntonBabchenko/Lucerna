import type { LoaderKind } from '$lib/ipc/bindings';

// Backend serialises LoaderKind as snake_case (vanilla, fabric, quilt,
// forge, neoforge). For UI we want the brand-canonical capitalisation,
// especially NeoForge which is intentionally PascalCase per their docs.
const DISPLAY: Record<LoaderKind, string> = {
  vanilla: 'Vanilla',
  fabric: 'Fabric',
  quilt: 'Quilt',
  forge: 'Forge',
  neoforge: 'NeoForge',
};

export function displayLoader(k: LoaderKind): string {
  return DISPLAY[k];
}

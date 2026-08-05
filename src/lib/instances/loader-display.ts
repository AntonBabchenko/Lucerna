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

// Display name for a raw loader tag as the mod platforms report it (Modrinth
// version `loaders`, FTB targets, the CurseForge tags the modpack client
// splits out). Unknown tags pass through verbatim — inventing a name would be
// worse than showing the platform's own.
export function displayLoaderTag(tag: string): string {
  const key = tag.trim().toLowerCase();
  return key in DISPLAY ? DISPLAY[key as LoaderKind] : tag;
}

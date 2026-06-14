import type { ForeignLauncher } from '$lib/ipc/bindings';

// Brand-canonical labels for the launcher an instance was imported from.
// Used by the Manage modal's provenance row. Kept out of i18n because these
// are proper product names, identical across locales.
const DISPLAY: Record<ForeignLauncher, string> = {
  prism: 'Prism Launcher',
  curseforge_app: 'CurseForge',
  modrinth_app: 'Modrinth App',
  atlauncher: 'ATLauncher',
  raw_minecraft: 'Minecraft',
  mojang_launcher: 'Minecraft Launcher',
  tlauncher: 'TLauncher',
};

export function displayLauncher(l: ForeignLauncher): string {
  return DISPLAY[l];
}

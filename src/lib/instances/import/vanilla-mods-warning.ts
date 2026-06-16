import type { ContentEntry, LoaderKind } from '$lib/ipc/bindings';

/**
 * True when the user is about to import a modded instance with no loader:
 * `loader` is Vanilla but the instance carries mod jars. Mods need a loader,
 * so Vanilla would load none — the wizard surfaces this as a warning.
 */
export function shouldWarnVanillaWithMods(loader: LoaderKind, content: ContentEntry[]): boolean {
  if (loader !== 'vanilla') return false;
  const mods = content.find((c) => c.category === 'mods');
  return (mods?.file_count ?? 0) > 0;
}

// The one warning for a library install whose same-name refresh left worlds
// behind. Installing a pack under a name the library already holds replaces
// it, and the worlds linked to the old copy are refreshed
// (`LibraryInstall.refreshed`). A world that could not be refreshed may still
// be on the old bytes, so every install path (catalog, local file, Vanilla
// Tweaks) names it here, in the same words, and so does an update whose new
// version kept the filename (`DatapackUpdateOutcome.old_copy_kept` false).
// Not `updateIncomplete`: that one promises a retry can finish, which holds
// only for a renamed update (the old library copy is kept) — here the library
// already holds the new bytes, so a retry reads the world's old file as not
// ours and skips it.
//
// The title names the pack and counts nothing. An install can hold many packs
// (a Vanilla Tweaks build, a drag-drop batch), each with its own warning, and
// only the name tells them apart. A count would also be wrong: a `saves/`
// folder that could not be listed is one entry, yet no world was checked at
// all, which its line says.
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { WorldMigration } from '$lib/ipc/bindings';
import { pushWarning } from '$lib/toasts/toasts.svelte';

/** One `world: details` line per world the refresh could not update. */
export function failedRefreshLines(refreshed: readonly WorldMigration[]): string[] {
  return refreshed.flatMap((m) => (m.kind === 'failed' ? [`${m.world}: ${m.details}`] : []));
}

/**
 * Warn about every world the refresh of `name` (the pack's display name, as
 * the library lists it) could not update; nothing when there is none.
 */
export function warnFailedRefresh(name: string, refreshed: readonly WorldMigration[]): void {
  const lines = failedRefreshLines(refreshed);
  if (lines.length === 0) return;
  pushWarning(get(t)('addons.datapacks.refreshIncomplete', { name }), lines);
}

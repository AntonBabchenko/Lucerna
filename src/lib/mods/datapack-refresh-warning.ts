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
// only the name tells them apart. A count would also be wrong: when no world
// could be checked at all (`worlds_unchecked`: a `saves/` folder that could
// not be listed) that is one entry, which its line says. `warnUpdateIncomplete`
// follows the same rule for the same reason.
//
// Every line is worded here, in the UI language: a failure carries the
// backend's typed error, which `formatError` renders like any other error.
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { WorldMigration } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { pushWarning } from '$lib/toasts/toasts.svelte';

/**
 * One `world: why` line per world the refresh could not update, and one line
 * saying the worlds could not be checked when none was.
 */
export function failedRefreshLines(refreshed: readonly WorldMigration[]): string[] {
  return refreshed.flatMap((m) => {
    switch (m.kind) {
      case 'failed':
        return [`${m.world}: ${formatError(m.error)}`];
      case 'worlds_unchecked':
        return [get(t)('addons.datapacks.worldsUnchecked', { error: formatError(m.error) })];
      default:
        return [];
    }
  });
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

/**
 * Warn that a renamed update of `name` did not complete. Only for an outcome
 * that kept the old library copy (`old_copy_kept`): the title promises that a
 * retry can finish, which holds only then.
 */
export function warnUpdateIncomplete(name: string, migrations: readonly WorldMigration[]): void {
  pushWarning(
    get(t)('addons.datapacks.updateIncomplete', { name }),
    failedRefreshLines(migrations),
  );
}

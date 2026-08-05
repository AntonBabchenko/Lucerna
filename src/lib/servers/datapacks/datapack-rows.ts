import type { ServerDatapackEntry } from '$lib/ipc/bindings';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { BadgeVariant } from '$lib/ui/cards/card-status';

export type RowBadge = {
  variant: BadgeVariant;
  labelKey: TranslationKey;
};

/**
 * Five cases, four badges: the four `WorldPackState` variants plus `null`.
 *
 * `orphaned` and `not_added` collapse onto ONE badge. The backend keeps them
 * distinct because they are distinct facts (an Enabled-list name whose file is
 * gone vs a Disabled-only one), but to an admin they are the same thing — a
 * level.dat entry with no file — and both are cleared the same way, by
 * removal. The label is server-scoped rather than reused from the client:
 * `worlds.datapacks.stateNotAdded` reads "Not in this world", which is
 * meaningful across N worlds and meaningless for a server that has one.
 */
export function badgeOf(entry: ServerDatapackEntry): RowBadge {
  switch (entry.state) {
    case 'enabled':
      return { variant: 'success', labelKey: 'worlds.datapacks.stateEnabled' };
    case 'disabled':
      return { variant: 'muted', labelKey: 'worlds.datapacks.stateDisabled' };
    case 'orphaned':
    case 'not_added':
      return { variant: 'danger', labelKey: 'servers.datapacks.stateGhost' };
    default:
      return { variant: 'neutral', labelKey: 'addons.datapacks.stateUnknown' };
  }
}

/**
 * The row identity, and the key of every per-row map in the pane.
 *
 * NOT sha1, which is what the plugin pane keys on: a folder pack is never
 * hashed and a ghost row has no file to hash, so both carry an empty sha1 and
 * a sha1 key would collide all of them onto `''`. The filename is the identity
 * the whole backend already uses, and the listing guarantees exactly one
 * spelling per pack.
 */
export function rowKey(entry: ServerDatapackEntry): string {
  return entry.record.filename.toLowerCase();
}

/**
 * Whether this row can carry an update affordance at all. A folder pack has no
 * provenance and is never produced by the catalog; a ghost has no file; a
 * hand-dropped zip has no platform identity to query.
 */
export function isUpdatable(entry: ServerDatapackEntry): boolean {
  return (
    entry.present &&
    !entry.is_folder &&
    entry.record.source !== null &&
    entry.record.project_id !== null
  );
}

import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { LevelDatPresence, ServerDatapackEntry } from '$lib/ipc/bindings';
import type { BadgeVariant } from '$lib/ui/cards/card-status';
import { ignoredLabelKey } from '$lib/worlds/datapack-state';

export type RowBadge = {
  variant: BadgeVariant;
  labelKey: TranslationKey;
};

/**
 * Six cases, five badges: the five `WorldPackState` variants plus `null`.
 * An `ignored` row (an entry the game does not load) gets a warning badge
 * whose label comes from its reason (spec §2 N.6).
 *
 * `orphaned` and `not_added` collapse onto ONE badge. The backend keeps them
 * distinct because they are distinct facts (an Enabled-list name whose file is
 * gone vs a Disabled-only one), but to an admin they are the same thing — a
 * level.dat entry with no file — and both are cleared the same way, by
 * removal. The label is server-scoped rather than reused from the client:
 * `worlds.datapacks.stateNotAdded` reads "Not in this world", which is
 * meaningful across N worlds and meaningless for a server that has one.
 *
 * Neutral, not danger: a ghost is not a problem — Minecraft skips the id and
 * drops it at its next save (spec 2026-09-24 §4 U3).
 */
export function badgeOf(entry: ServerDatapackEntry): RowBadge {
  switch (entry.state) {
    case 'enabled':
      return { variant: 'success', labelKey: 'worlds.datapacks.stateEnabled' };
    case 'disabled':
      return { variant: 'muted', labelKey: 'worlds.datapacks.stateDisabled' };
    case 'orphaned':
    case 'not_added':
      return { variant: 'neutral', labelKey: 'servers.datapacks.stateGhost' };
    case 'ignored':
      return { variant: 'warning', labelKey: ignoredLabelKey(entry.ignored_reason) };
    default:
      return { variant: 'neutral', labelKey: 'addons.datapacks.stateUnknown' };
  }
}

/**
 * The row identity, and the key of every per-row map in the pane.
 *
 * NOT sha1, which is what the plugin pane keys on: a folder pack is never
 * hashed and a ghost row has no file to hash, so both carry an empty sha1 and
 * a sha1 key would collide all of them onto `''`. The exact filename (spec §2
 * N.3): on a case-sensitive file system `Foo.zip` and `foo.zip` are two
 * packs, and the listing keeps one row per on-disk spelling.
 */
export function rowKey(entry: ServerDatapackEntry): string {
  return entry.record.filename;
}

/**
 * Whether this row can carry an update affordance at all. A folder pack has no
 * provenance and is never produced by the catalog; a ghost has no file; a
 * hand-dropped zip has no platform identity to query; an entry the game
 * ignores is not a pack it loads.
 */
export function isUpdatable(entry: ServerDatapackEntry): boolean {
  return (
    entry.state !== 'ignored' &&
    entry.present &&
    !entry.is_folder &&
    entry.record.source !== null &&
    entry.record.project_id !== null
  );
}

/**
 * Why EVERY change to this server world's packs is off, or null (D2): a world
 * with only level.dat_old is restored by the server's next start, and Lucerna
 * must not pre-empt that. A never-started world (`absent`) keeps add, update
 * and remove (spec 2026-09-24 I4); see {@link serverToggleBlockedKey}.
 */
export function serverWorldBlockedKey(levelDat: LevelDatPresence | null): TranslationKey | null {
  return levelDat === 'only_old' ? 'servers.datapacks.blockedOnlyOld' : null;
}

/**
 * Why the on/off toggle alone is off, or null. On a never-started world it is
 * rendered disabled up front (the backend refuses with ServerWorldNotCreated
 * anyway): the state lives in a level.dat the server has not written yet.
 */
export function serverToggleBlockedKey(levelDat: LevelDatPresence | null): TranslationKey | null {
  switch (levelDat) {
    case 'only_old':
      return 'servers.datapacks.blockedOnlyOld';
    case 'absent':
      return 'servers.datapacks.blockedNotCreated';
    case 'present':
    case null:
      return null;
  }
}

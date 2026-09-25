import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { PackCompat, WontLoadReason } from '$lib/ipc/bindings';

/**
 * The sentence a compatibility verdict earns, as a key plus its values (the
 * caller renders it with `$t`, so a live language switch re-renders it).
 * One source for the world tab and the library row (§1 C5). The copy
 * follows the game's own `pack.incompatible.*`: a too-old, too-new or
 * "broken" pack still LOADS and "may not work correctly"; only `wont_load`
 * is skipped.
 */
export interface CompatLine {
  key: TranslationKey;
  args: Record<string, string>;
}

/** The key naming why this version skips a pack. Exhaustive: a new backend
 *  reason fails svelte-check here. */
export function wontLoadKey(reason: WontLoadReason): TranslationKey {
  switch (reason) {
    case 'no_pack_mcmeta':
      return 'worlds.datapacks.compatWontLoadNoPackMcmeta';
    case 'no_pack_section':
      return 'worlds.datapacks.compatWontLoadNoPackSection';
    case 'no_description':
      return 'worlds.datapacks.compatWontLoadNoDescription';
    case 'no_pack_format':
      return 'worlds.datapacks.compatWontLoadNoPackFormat';
    default: {
      const _exhaustive: never = reason;
      return _exhaustive;
    }
  }
}

/** `null` when there is nothing to warn about (`compatible`) or nothing to
 *  claim (`unknown`, which keeps its own neutral badge). */
export function compatLine(c: PackCompat): CompatLine | null {
  switch (c.kind) {
    case 'too_old':
      return { key: 'worlds.datapacks.compatTooOld', args: { madeFor: c.made_for, game: c.game } };
    case 'too_new':
      return { key: 'worlds.datapacks.compatTooNew', args: { madeFor: c.made_for, game: c.game } };
    case 'broken':
      return { key: 'worlds.datapacks.compatBroken', args: {} };
    case 'wont_load':
      return { key: wontLoadKey(c.reason), args: {} };
    case 'compatible':
    case 'unknown':
      return null;
    default: {
      const _exhaustive: never = c;
      return _exhaustive;
    }
  }
}

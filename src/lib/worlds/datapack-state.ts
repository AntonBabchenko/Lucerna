import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { IgnoredReason, PackCompat } from '$lib/ipc/bindings';
import { wontLoadKey } from './datapack-compat';

/**
 * The badge label for a row whose state is `ignored`. `unreadable` is not a
 * claim about the game (Lucerna could not read the entry), so it says
 * "Couldn't check" (spec §2 N.1, fallback honesty). `null` cannot arrive
 * while the backend keeps `state === 'ignored' ⟹ ignored_reason !== null`.
 * (The converse no longer holds for a library placement: one Lucerna could
 * not check at all is `state: null` + `unreadable`, and never reaches here.)
 */
export function ignoredLabelKey(reason: IgnoredReason | null): TranslationKey {
  switch (reason) {
    case 'unreadable':
      return 'worlds.datapacks.stateCouldNotCheck';
    case null:
    case 'folder_without_pack_mcmeta':
    case 'folder_pack_nested_inside':
    case 'zip_extension_not_lowercase':
    case 'zip_without_pack_mcmeta':
    case 'not_loadable':
      return 'worlds.datapacks.stateIgnored';
    default: {
      const _exhaustive: never = reason;
      return _exhaustive;
    }
  }
}

/** The hint line under an ignored row (§0.5 A12). `null` = no hint. */
export function ignoredHintKey(
  reason: IgnoredReason | null,
  compat?: PackCompat,
): TranslationKey | null {
  switch (reason) {
    case null:
      return null;
    case 'folder_without_pack_mcmeta':
      return 'worlds.datapacks.ignoredReason.folderWithoutPackMcmeta';
    case 'folder_pack_nested_inside':
      return 'worlds.datapacks.ignoredReason.folderPackNestedInside';
    case 'zip_extension_not_lowercase':
      return 'worlds.datapacks.ignoredReason.zipExtensionNotLowercase';
    case 'zip_without_pack_mcmeta':
      return 'worlds.datapacks.ignoredReason.zipWithoutPackMcmeta';
    case 'unreadable':
      return 'worlds.datapacks.ignoredReason.unreadable';
    case 'not_loadable':
      return notLoadableHintKey(compat);
    default: {
      const _exhaustive: never = reason;
      return _exhaustive;
    }
  }
}

/**
 * `not_loadable` has no key of its own (§0.2 I3). The backend derives it from
 * the row's own `wont_load` verdict (`state::loadable_of`), so its hint is
 * that verdict's `worlds.datapacks.compatWontLoad*` line, naming the exact
 * missing field (§0.5 A12). Without a `wont_load` verdict to name, it says
 * only that compatibility is unknown — never a guessed cause.
 */
export function notLoadableHintKey(compat: PackCompat | undefined): TranslationKey {
  return compat?.kind === 'wont_load'
    ? wontLoadKey(compat.reason)
    : 'worlds.datapacks.compatUnknown';
}

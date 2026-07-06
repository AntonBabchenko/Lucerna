import type { TranslationKey } from '$lib/i18n/keys.generated';

/**
 * When the configured data root is unavailable, the app runs against a
 * temporary default location (see §7 of the data-root design doc). Creating
 * new instances/servers or launching Minecraft while fallen back would write
 * data into the WRONG root — split-brain data once the real drive returns —
 * so those actions are blocked and this key explains why. Returns `null`
 * (not blocked) otherwise. Mirrors `quickPlayDisabledKey`'s single-source-
 * of-truth shape.
 */
export function dataRootCreateDisabledKey(fellBack: boolean): TranslationKey | null {
  return fellBack ? 'page.dataRootFallback.createDisabledReason' : null;
}

export function dataRootPlayDisabledKey(fellBack: boolean): TranslationKey | null {
  return fellBack ? 'page.dataRootFallback.playDisabledReason' : null;
}

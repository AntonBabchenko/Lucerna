import type { TranslationKey } from '$lib/i18n/keys.generated';

export interface QuickPlayState {
  ready: boolean;
  running: boolean;
  supported: boolean;
}

/**
 * The i18n key for why Quick Play is disabled, or `null` when available.
 * Priority: not-installed → running → version-unsupported. Single source of
 * truth shared by +page (tooltip text) and the gating test.
 */
export function quickPlayDisabledKey(s: QuickPlayState): TranslationKey | null {
  if (!s.ready) return 'worlds.quickPlay.disabledNotReady';
  if (s.running) return 'worlds.quickPlay.disabledRunning';
  if (!s.supported) return 'worlds.quickPlay.disabledUnsupported';
  return null;
}

// The tray's words, out of +page.svelte so they can be tested (+page.svelte
// cannot be mounted in isolation). The backend owns the tray but not the
// language; the page sends these through traySetLabels.

import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { RestartBlock, TrayLabels } from '$lib/ipc/bindings';

export type Translate = (key: TranslationKey) => string;

/** The three menu strings, in the interface language. */
export function trayLabels(_t: Translate): TrayLabels {
  // STUB (red).
  return { open: '', quit: '', tooltip_running: '' };
}

/** The warning shown when a tray Quit was refused, per reason. */
export function trayRefusalKey(_block: Exclude<RestartBlock, 'none'>): TranslationKey {
  // STUB (red).
  return 'tray.open';
}

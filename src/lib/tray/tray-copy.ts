// The tray's words, out of +page.svelte so they can be tested (+page.svelte
// cannot be mounted in isolation). The backend owns the tray but not the
// language; the page sends these through traySetLabels.

import type { Translate } from '$lib/i18n';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { RestartBlock, TrayLabels } from '$lib/ipc/bindings';

/** The three menu strings, in the interface language. */
export function trayLabels(t: Translate): TrayLabels {
  return {
    open: t('tray.open'),
    quit: t('tray.quit'),
    tooltip_running: t('tray.tooltipRunning'),
  };
}

/** The warning shown when a tray Quit was refused, per reason. */
export function trayRefusalKey(block: Exclude<RestartBlock, 'none'>): TranslationKey {
  switch (block) {
    case 'running':
      return 'tray.blocked.running';
    case 'busy':
      return 'tray.blocked.busy';
    case 'unknown':
      return 'tray.blocked.unknown';
    default: {
      // A new reason must get its own sentence, not borrow another's.
      const unhandled: never = block;
      return unhandled;
    }
  }
}

// What the close dialog says. One line per kind of loss, so the user reads
// exactly what closing Lucerna would take down — and nothing it would not.

import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { CloseLabels, CloseLosses } from '$lib/ipc/bindings';

export type Translate = (
  key: TranslationKey,
  options?: { values?: Record<string, string | number> },
) => string;

/** One line per set kind, in a fixed order: games, servers, operation, unchecked. */
export function closeLines(_losses: CloseLosses, _t: Translate): string[] {
  // STUB (red).
  return [];
}

/**
 * "Close everything" when closing kills something; "Close Lucerna" when the
 * only thing set is `unchecked` — then nothing is being killed, and a verb
 * that says so would be false.
 */
export function closeConfirmLabel(_losses: CloseLosses, _t: Translate): string {
  // STUB (red).
  return '';
}

/** Destructive styling only when something is actually killed. */
export function closeConfirmVariant(_losses: CloseLosses): 'danger' | 'primary' {
  // STUB (red).
  return 'primary';
}

/**
 * The native fallback's words, in the interface language. Plural-free: the
 * backend picks the one-server or many-servers line itself.
 */
export function nativeCloseLabels(_t: Translate): CloseLabels {
  // STUB (red).
  return {
    title: '',
    close_everything: '',
    close_lucerna: '',
    cancel: '',
    games: '',
    servers_one: '',
    servers_many: '',
    operation: '',
    unchecked: '',
  };
}

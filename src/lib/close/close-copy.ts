// What the close dialog says. One line per kind of loss, so the user reads
// exactly what closing Lucerna would take down — and nothing it would not.

// The project's own translator type: values go in directly, t(key, { count }).
import type { Translate } from '$lib/i18n';
import type { CloseLabels, CloseLosses } from '$lib/ipc/bindings';

/** Whether confirming kills something. `unchecked` alone does not: the exit
 *  hook cannot stop a server it could not identify. */
function kills(losses: CloseLosses): boolean {
  return losses.games || losses.servers > 0 || losses.operation;
}

/** One line per set kind, in a fixed order: games, servers, operation, unchecked. */
export function closeLines(losses: CloseLosses, t: Translate): string[] {
  const lines: string[] = [];
  if (losses.games) lines.push(t('closeConfirm.games'));
  if (losses.servers > 0) lines.push(t('closeConfirm.servers', { count: losses.servers }));
  if (losses.operation) lines.push(t('closeConfirm.operation'));
  if (losses.unchecked) lines.push(t('closeConfirm.unchecked'));
  return lines;
}

/**
 * "Close everything" when closing kills something; "Close Lucerna" when the
 * only thing set is `unchecked` — then nothing is being killed, and a verb
 * that says so would be false.
 */
export function closeConfirmLabel(losses: CloseLosses, t: Translate): string {
  return kills(losses) ? t('closeConfirm.closeEverything') : t('closeConfirm.closeLucerna');
}

/** Destructive styling only when something is actually killed. */
export function closeConfirmVariant(losses: CloseLosses): 'danger' | 'primary' {
  return kills(losses) ? 'danger' : 'primary';
}

/**
 * The native fallback's words, in the interface language. Plural-free: the
 * backend picks the one-server or many-servers line itself.
 */
export function nativeCloseLabels(t: Translate): CloseLabels {
  return {
    title: t('closeConfirm.title'),
    close_everything: t('closeConfirm.closeEverything'),
    close_lucerna: t('closeConfirm.closeLucerna'),
    cancel: t('common.cancel'),
    games: t('closeConfirm.games'),
    servers_one: t('closeConfirm.nativeServersOne'),
    servers_many: t('closeConfirm.nativeServersMany'),
    operation: t('closeConfirm.operation'),
    unchecked: t('closeConfirm.unchecked'),
  };
}

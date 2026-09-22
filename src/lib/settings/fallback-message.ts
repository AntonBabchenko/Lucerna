// What a recovery session tells the user, in ONE place: the banner and the compact-mode notice
// both render it, so the two cannot drift apart.
//
// The launcher starts a recovery session when it cannot use the configured data folder. WHY
// matters: "reconnect it" is a lie about a folder that is plugged in and merely read-only, and a
// corrupt pointer has no path to name at all. The backend reports the reason; this picks the
// sentence. Details (an OS error string) are deliberately left out — they belong to the Storage
// notice, not to a banner the user reads at a glance.

import type { Translate } from '$lib/i18n';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { DataLocationStatus, Fallback } from '$lib/ipc/bindings';

// A Record, so a new `Fallback` kind is a compile error here.
const REASON_KEYS: Record<Fallback['kind'], TranslationKey> = {
  root_missing: 'page.dataRootFallback.reason.rootMissing',
  root_not_writable: 'page.dataRootFallback.reason.rootNotWritable',
  root_unknown: 'page.dataRootFallback.reason.rootUnknown',
  pointer_unreadable: 'page.dataRootFallback.reason.pointerUnusable',
  pointer_corrupt: 'page.dataRootFallback.reason.pointerUnusable',
};

/** Statuses as older mocks and six unit suites still build them: `fell_back` without a reason. */
type StatusLike = Pick<DataLocationStatus, 'fell_back'> & { fallback?: Fallback | null };

/** Why the launcher is in a recovery session; `null` when it is not. An old-shaped status
 *  (`fell_back: true`, no `fallback`) reads as a missing folder — the only reason that existed
 *  before reasons did — so it neither crashes the UI nor hides the fallback. */
export function fallbackOf(status: StatusLike | null): Fallback | null {
  if (!status) return null;
  return status.fallback ?? (status.fell_back ? { kind: 'root_missing' } : null);
}

/** The reason sentence plus the shared "temporary session" tail. `configured` is null for the
 *  two pointer reasons, whose sentence has no `{path}`. */
export function fallbackMessage(t: Translate, reason: Fallback, configured: string | null): string {
  const sentence = t(REASON_KEYS[reason.kind], { path: configured ?? '' });
  return `${sentence} ${t('page.dataRootFallback.temporary')}`;
}

/** The OS error behind the reason, when there is one — for the Storage notice. */
export function fallbackDetails(reason: Fallback): string | null {
  return 'details' in reason ? reason.details : null;
}

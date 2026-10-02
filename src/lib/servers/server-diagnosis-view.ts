// Pure view helpers for the server diagnosis banner: when it should render and
// how to identify the current diagnosis for dismissal. Kept as a module so the
// banner (which renders it) and ServersPanel (which renders the restore
// badge) agree on one definition instead of duplicating the condition.

import type { Error as IpcError, ServerDiagnosis } from '$lib/ipc/bindings';
import { isIpcError } from '$lib/ipc/format-error';

/**
 * Whether the diagnosis banner is eligible to show: ServerDiagnosisBanner's
 * `{#if}` is this predicate and "not dismissed" (its `diag?.diagnosis` test is
 * there for type narrowing only), and ServersPanel reads the same two for the
 * restore badge and for whether the banner on screen covers an action error.
 */
export function serverBannerEligible(
  diag: ServerDiagnosis | null | undefined,
  running: boolean,
): boolean {
  return !!(diag?.diagnosis && diag.status !== 'none' && diag.status !== 'handled' && !running);
}

/**
 * The banner that states a refused action's error itself, with its fix attached, by the error's
 * kind. A refusal is otherwise answered by its own message: the banner describes the LAST RUN (its
 * log, crash report or exit code) or a blocker found before the next one, never the refusal — a
 * server refused before it started keeps the previous run's log, so its banner may well be about an
 * old crash. A refused start without an accepted EULA is the one refusal a banner repeats.
 */
const BANNER_FOR_ERROR: Partial<Record<IpcError['kind'], string>> = {
  server_eula_not_accepted: 'server-eula-not-accepted',
};

/**
 * Whether the diagnosis banner says what `error` — a refused start, stop or restart, as the store
 * holds it — says. Only then does the panel leave the error out; the caller still asks whether the
 * banner is on screen at all.
 */
export function bannerExplainsActionError(
  diag: ServerDiagnosis | null | undefined,
  error: unknown,
): boolean {
  if (!isIpcError(error) || !diag?.diagnosis) return false;
  const pattern = BANNER_FOR_ERROR[error.kind];
  return pattern !== undefined && diag.diagnosis.pattern_id === pattern;
}

/**
 * Stable identity for the current server diagnosis, used as the dismissal
 * signature. `pattern_id` is always present when the banner is eligible;
 * `log_signature` (null for pre-spawn diagnoses like port/EULA/orphan)
 * disambiguates same-pattern-different-log crashes. Null when there is no
 * diagnosis to identify.
 */
export function serverDiagnosisSignature(diag: ServerDiagnosis | null | undefined): string | null {
  if (!diag?.diagnosis) return null;
  return `${diag.diagnosis.pattern_id}|${diag.log_signature ?? ''}`;
}

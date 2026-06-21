// Pure view helpers for the server diagnosis banner: when it should render and
// how to identify the current diagnosis for dismissal. Kept as a module so the
// banner (which renders it) and ServerManageView (which renders the restore
// badge) agree on one definition instead of duplicating the condition.

import type { ServerDiagnosis } from '$lib/ipc/bindings';

/**
 * Whether the diagnosis banner is eligible to show. Mirrors the `{#if}` in
 * ServerDiagnosisBanner.svelte — keep the two in sync. (The banner keeps the
 * condition inline so Svelte narrows `diag` to non-null in its body; this helper
 * is the same predicate for surfaces that only need the boolean, e.g. the
 * restore badge.)
 */
export function serverBannerEligible(
  diag: ServerDiagnosis | null | undefined,
  running: boolean,
): boolean {
  return !!(diag?.diagnosis && diag.status !== 'none' && diag.status !== 'handled' && !running);
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

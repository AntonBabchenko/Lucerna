// Pure view helpers for the log diagnosis banner: when it should render and how
// to identify the current diagnosis for dismissal. Shared by LogDiagnosisBanner
// (renders the banner) and LogsPopover (renders the restore badge) so both read
// one definition.

import type { LatestDiagnosis } from '$lib/ipc/bindings';

/**
 * Whether the log diagnosis banner is eligible to show. Mirrors the `{#if}` in
 * LogDiagnosisBanner.svelte — keep the two in sync.
 */
export function logBannerEligible(latest: LatestDiagnosis | null): boolean {
  return !!(latest?.diagnosis && (latest.status === 'actionable' || latest.status === 'advisory'));
}

/**
 * Stable identity for the current log diagnosis, used as the dismissal
 * signature. Prefers the backend log-content `signature`; falls back to the
 * pattern id when the signature is null. Null when there is no diagnosis.
 */
export function logDiagnosisSignature(latest: LatestDiagnosis | null): string | null {
  if (!latest?.diagnosis) return null;
  return latest.signature ?? latest.diagnosis.pattern_id;
}

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
 *
 * Accepted limitation: the `pattern_id` fallback is the best available identity
 * for a log-less diagnosis but does not distinguish repeated occurrences of the
 * same pattern — a dismissed pattern stays hidden if it recurs without a
 * signature. In practice diagnoses carry a content signature; this is the rare
 * degenerate path, not the norm.
 */
export function logDiagnosisSignature(latest: LatestDiagnosis | null): string | null {
  if (!latest?.diagnosis) return null;
  return latest.signature ?? latest.diagnosis.pattern_id;
}

/**
 * Whether the per-file inline diagnosis card duplicates the latest-log banner,
 * so the card should collapse to just its raw `matched_excerpt` (the only thing
 * the banner does not already show) instead of repeating title/explanation/
 * recommendation.
 *
 * Redundant ⇔ the banner is visible (eligible AND not dismissed) AND the
 * selected file is the banner's own file. Same file ⇒ same diagnose result, so
 * path equality is an exact match — no need to compare pattern_id/signature. If
 * the banner is dismissed or not eligible there is no banner above, so the card
 * must stay full (it is then the only place the diagnosis is shown).
 */
export function inlineDiagnosisRedundant(
  selectedPath: string | null,
  latest: LatestDiagnosis | null,
  bannerDismissed: boolean,
): boolean {
  return (
    selectedPath !== null &&
    !bannerDismissed &&
    logBannerEligible(latest) &&
    latest?.path === selectedPath
  );
}

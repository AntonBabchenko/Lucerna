// Pure shaping for the pre-fill summary. No DOM, no IPC — same split as
// coverage.ts and key-rows.ts.
//
// `RunSummary` carries three facts the dialog must not flatten into a single
// "it worked" line, and each of them is a different claim:
//
//   * `usage_known === false` means NO provider reported token usage (a local
//     model typically reports none). "Cost unknown" and "cost zero" are not
//     the same statement, so this maps to `null` rather than to zeros.
//   * `pack_rebuilt === false` means the strings are on disk but not in the
//     resource pack — e.g. the user launched the instance mid-run. The run
//     succeeded; the last step did not.
//   * `failed` means the run stopped early, AFTER buying and flushing
//     everything the summary counts. Reporting it as a bare error would throw
//     away the honest report of what the user already paid for.
import type { RunSummary } from '$lib/ipc/bindings';

export type SummaryView = {
  /** 'warning' when something needs the user's attention afterwards — a
   *  stopped run or an unbuilt pack. Cancelling is the user's own decision,
   *  so it is not by itself a warning. */
  tone: 'success' | 'warning';
  written: number;
  fromCache: number;
  fromGlossary: number;
  rejected: number;
  cancelled: boolean;
  /** `null` when no completion reported usage — show nothing, never a zero. */
  tokens: { prompt: number; completion: number } | null;
  /** Set when the strings were written but the pack rebuild failed. */
  packError: { message: string | null } | null;
  /** Set when the run stopped early; the counts above still hold. */
  failed: string | null;
};

export function summaryView(summary: RunSummary): SummaryView {
  return {
    tone: summary.failed !== null || !summary.packRebuilt ? 'warning' : 'success',
    written: summary.written,
    fromCache: summary.fromCache,
    fromGlossary: summary.fromGlossary,
    rejected: summary.rejected,
    cancelled: summary.cancelled,
    tokens: summary.usageKnown
      ? { prompt: summary.promptTokens, completion: summary.completionTokens }
      : null,
    // `pack_rebuild_error` can be null even when the rebuild failed, so the
    // caveat is keyed off the boolean and carries the message as an optional
    // detail rather than being derived from the message's presence.
    packError: summary.packRebuilt ? null : { message: summary.packRebuildError },
    failed: summary.failed,
  };
}

import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { displayLoader } from '$lib/instances/loader-display';
import type { LoaderKind, LoaderOutcome, ModLocalCompat } from '$lib/ipc/bindings';
import { isOfflineMismatch } from '$lib/mods/compat-scan.svelte';

// ---------------------------------------------------------------------------
// loaderOutcomeToast
// ---------------------------------------------------------------------------

export type OutcomeToast = { kind: 'success'; text: string } | { kind: 'warning'; text: string };

/**
 * Map a LoaderOutcome from changeInstanceMc to a toast descriptor, or null
 * when no toast is needed (unchanged). Pure — no side-effects, easy to test.
 */
export function loaderOutcomeToast(outcome: LoaderOutcome, mc: string): OutcomeToast | null {
  if (outcome.kind === 'unchanged') return null;

  if (outcome.kind === 'loader_updated') {
    const { loader, version } = outcome as {
      kind: 'loader_updated';
      loader: LoaderKind;
      version: string;
    };
    return {
      kind: 'success',
      text: get(t)('instance.integrity.loaderUpdated', {
        loader: displayLoader(loader),
        version,
        mc,
      }),
    };
  }

  // loader_reset_to_vanilla
  const { previous_loader } = outcome as {
    kind: 'loader_reset_to_vanilla';
    previous_loader: LoaderKind;
  };
  return {
    kind: 'warning',
    text: get(t)('instance.integrity.loaderResetToVanilla', {
      loader: displayLoader(previous_loader),
      mc,
    }),
  };
}

// ---------------------------------------------------------------------------
// compatSummaryFromScan
// ---------------------------------------------------------------------------

/**
 * Summarise the offline compat scan into a single warning string, or null
 * when nothing was scanned or every scanned mod is fine. Pure — no
 * side-effects.
 *
 * This replaces the old `compatSummary(rows: ModCompat[])`, which asked the
 * platform "does this PROJECT have a build for (mc, loader)?" — a question
 * every project can answer yes to even when the FILE on disk is a stale
 * build from before a version change. `entries` comes from the offline scan
 * (`scanInstanceModCompat` / `ensureCompatScan`), which judges the file
 * itself, via the same `isOfflineMismatch` predicate the Overview's
 * `offlineMismatchCount` uses — one verdict, not two.
 *
 * There is no "unknown" bucket here the way there was for live platform
 * queries: the offline scan is unconditional, every installed mod gets a
 * definite mismatch/fine verdict, so `compatWarningWithUnknown` has no
 * caller left once this replaces `compatSummary`.
 */
export function compatSummaryFromScan(entries: ModLocalCompat[], total: number): string | null {
  if (entries.length === 0) return null;

  const incompatible = entries.filter(isOfflineMismatch).length;
  if (incompatible === 0) return null;

  return get(t)('instance.integrity.compatWarning', { incompatible, total });
}

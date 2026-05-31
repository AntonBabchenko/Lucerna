import { displayLoader } from '$lib/instances/loader-display';
import type { LoaderKind, LoaderOutcome, ModCompat } from '$lib/ipc/bindings';

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
      text: `${displayLoader(loader)} updated to ${version} for Minecraft ${mc}`,
    };
  }

  // loader_reset_to_vanilla
  const { previous_loader } = outcome as {
    kind: 'loader_reset_to_vanilla';
    previous_loader: LoaderKind;
  };
  return {
    kind: 'warning',
    text: `No ${displayLoader(previous_loader)} build for Minecraft ${mc} — loader reset to Vanilla`,
  };
}

// ---------------------------------------------------------------------------
// compatSummary
// ---------------------------------------------------------------------------

/**
 * Summarise mod-compat results into a single warning string, or null when
 * every mod is compatible (or there are no mods). Pure — no side-effects.
 */
export function compatSummary(rows: ModCompat[]): string | null {
  if (rows.length === 0) return null;

  const incompatible = rows.filter((r) => r.status.status === 'incompatible').length;
  const unknown = rows.filter((r) => r.status.status === 'unknown').length;
  const total = rows.length;

  if (incompatible === 0 && unknown === 0) return null;

  const unknownSuffix = unknown > 0 ? ` · ${unknown} couldn't be checked` : '';

  return `${incompatible} of ${total} installed mods have no version for this configuration${unknownSuffix}. Review in the Mods tab.`;
}

import type { ModSummary, ModUpdateState, ModVersion } from '$lib/ipc/bindings';
import { modProjectUrl } from '$lib/mods/project-url';

/** The `update_available` variant of the shared update-state union. */
type UpdateAvailable = Extract<ModUpdateState, { kind: 'update_available' }>;

/** True iff `state` offers a newer version (auto-updatable OR external). Drives
 *  the Updates filter chip — an externally-hosted update still "has an update". */
export function hasUpdate(state: ModUpdateState | null | undefined): state is UpdateAvailable {
  return state?.kind === 'update_available';
}

/** True iff `state`'s target can be auto-installed (a hosted, distributable
 *  file). Hangar externally-hosted targets (`distribution_allowed === false`)
 *  are excluded — those open the project page instead of downloading.
 *
 *  Plain `boolean`, NOT a type guard: it must not re-narrow `state` in a
 *  caller's negative branch. A guard here would collapse `!isAutoUpdatable`
 *  to `never` after `hasUpdate` already narrowed to `UpdateAvailable`. */
export function isAutoUpdatable(state: ModUpdateState | null | undefined): boolean {
  return hasUpdate(state) && state.target.primary_file.distribution_allowed;
}

/** The external page URL for a non-distributable target: the file's own URL
 *  (Hangar's external download page) when present, else the project page derived
 *  from `summary`. `null` when neither is available. */
export function externalUpdateUrl(target: ModVersion, summary: ModSummary | null): string | null {
  if (target.primary_file.url.length > 0) return target.primary_file.url;
  if (summary) {
    return modProjectUrl(summary.source, summary.slug ?? summary.project_id, summary.author);
  }
  return null;
}

/** The `{ sha, target }` pairs to auto-update in an "Update all" batch: every
 *  row whose check is auto-updatable (external rows excluded). */
export function autoUpdateTargets<T extends { sha1: string }>(
  rows: readonly T[],
  checks: ReadonlyMap<string, ModUpdateState>,
): Array<{ sha: string; target: ModVersion }> {
  const out: Array<{ sha: string; target: ModVersion }> = [];
  for (const row of rows) {
    const st = checks.get(row.sha1);
    // `hasUpdate` narrows `st` to `UpdateAvailable` so `st.target` is typed;
    // the flag check is `isAutoUpdatable`'s body inlined for that narrowing.
    if (hasUpdate(st) && st.target.primary_file.distribution_allowed) {
      out.push({ sha: row.sha1, target: st.target });
    }
  }
  return out;
}

/** Count of rows with an auto-updatable pending update — drives the
 *  "Update all ({count})" label + enablement. */
export function countAutoUpdatable<T extends { sha1: string }>(
  rows: readonly T[],
  checks: ReadonlyMap<string, ModUpdateState>,
): number {
  return autoUpdateTargets(rows, checks).length;
}

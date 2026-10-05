import { get } from 'svelte/store';
import { type ReportLine, reasonLines } from '$lib/format/reason-lines';
import { type Translate, t } from '$lib/i18n';
import type { InstallSummary, ModSource, ModUpdateCheck, ModVersion } from '$lib/ipc/bindings';
import { changelogSupported } from '$lib/mods/changelog-supported';
import { modWriteReason } from '$lib/mods/mod-ops.svelte';
import { updateMod } from '$lib/tasks/adapters/mod-install';
import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
import type { Row } from './installed-data.svelte';
import { rowDisplayName } from './row-utils';

/** One pending update as the review dialog lists it (spec §6.6). */
export type UpdateReviewItem = {
  sha1: string;
  name: string;
  from: string | null;
  to: string;
  /** Null when the source publishes no changelog — the same gate as the row's badge. */
  changelog: {
    source: ModSource;
    projectId: string;
    targetVersionId: string;
    baseVersionId: string;
  } | null;
};

/** Every row with a pending update, by the name the tab shows for it. */
export function buildReviewItems(
  rows: readonly Row[],
  checks: ReadonlyMap<string, ModUpdateCheck>,
): UpdateReviewItem[] {
  const out: UpdateReviewItem[] = [];
  for (const row of rows) {
    const c = checks.get(row.installed.sha1);
    if (!c || c.state.kind !== 'update_available') continue;
    const { source, project_id, version_id, version_number } = row.installed;
    out.push({
      sha1: row.installed.sha1,
      name: rowDisplayName(row),
      from: version_number,
      to: c.state.target.version_number,
      changelog:
        source && project_id && version_id && changelogSupported(source)
          ? {
              source,
              projectId: project_id,
              targetVersionId: c.state.target.version_id,
              baseVersionId: version_id,
            }
          : null,
    });
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

/** One update to run: the jar it replaces, the name the tab shows for it, the build to install. */
export type UpdateTarget = { sha1: string; name: string; target: ModVersion };

/** How one update ended. A failure carries its reason, already worded for the user, and
 *  `previousKept`: the old version is known to be in place (`oldVersionKept`) — false, too, when
 *  the outcome is unknown (the bridge failed). */
export type UpdateAttempt =
  | { sha1: string; name: string; ok: true; summary: InstallSummary }
  | { sha1: string; name: string; ok: false; reason: string; previousKept: boolean };

/** Errors after which the old version is NOT known to be in place (see `oldVersionKept`). */
const OLD_VERSION_NOT_KNOWN: ReadonlySet<string> = new Set([
  'instance_busy',
  'content_update_unfinished',
  'content_update_rollback_incomplete',
]);

/**
 * Whether a failed `mods_update_one` left the old version in place. Once it holds the profile, any
 * error it returns means its files were never touched or `update_one`'s transaction put them back,
 * except where the error says otherwise: the undo left files behind
 * (`content_update_rollback_incomplete`), or an earlier update is unfinished and may hold files
 * aside (`content_update_unfinished`). A refusal (`instance_busy`) never held the profile: another
 * operation owns it and its files may be moving, so nothing is claimed. Nor for an error the
 * backend does not send (no `kind`).
 */
export function oldVersionKept(error: unknown): boolean {
  if (typeof error !== 'object' || error === null) return false;
  const kind = (error as { kind?: unknown }).kind;
  return typeof kind === 'string' && !OLD_VERSION_NOT_KNOWN.has(kind);
}

/**
 * Run the updates one after another; each gets its try whatever the one before did. Never throws.
 * An update is a mod write under the shared claim, so a refusal says the profile is busy — never
 * that the game runs (plan A9).
 */
export async function runUpdates(
  instanceId: string,
  targets: readonly UpdateTarget[],
): Promise<UpdateAttempt[]> {
  const attempts: UpdateAttempt[] = [];
  for (const { sha1, name, target } of targets) {
    try {
      const r = await updateMod(instanceId, name, sha1, target);
      attempts.push(
        r.status === 'ok'
          ? { sha1, name, ok: true, summary: r.data }
          : {
              sha1,
              name,
              ok: false,
              reason: modWriteReason(r.error),
              previousKept: oldVersionKept(r.error),
            },
      );
    } catch (e) {
      // The bridge failed on this call: a failure with its message, never read as success.
      attempts.push({
        sha1,
        name,
        ok: false,
        reason: e instanceof Error ? e.message : String(e),
        // Unknown: whether anything ran is not known, so nothing is claimed.
        previousKept: false,
      });
    }
  }
  return attempts;
}

/** The jars that were replaced: their rows in the check describe nothing any more. */
export function updatedShas(attempts: readonly UpdateAttempt[]): string[] {
  return attempts.flatMap((a) => (a.ok ? [a.sha1] : []));
}

/**
 * The dependencies an install or update brought in. Read defensively: the summary is a report,
 * and a malformed one must never turn a finished update into a failure (the task adapter reads its
 * `details` the same way).
 */
export function depsOf(summary: InstallSummary | null | undefined): string[] {
  return summary?.installed_dependencies ?? [];
}

/** «+ установлено: …» — every dependency these installs or updates brought in, once (D9). */
export function depsLines(
  tr: Translate,
  summaries: readonly (InstallSummary | null | undefined)[],
): string[] {
  const names = [...new Set(summaries.flatMap(depsOf))];
  return names.length > 0 ? [tr('mods.updates.installedDeps', { names: names.join(', ') })] : [];
}

export type UpdatesReportOpts = {
  /** A row's own update: a failure is named by the mod («Не удалось обновить X»). */
  single?: boolean;
  /** The profile the run was for, when the user has switched to another one meanwhile. */
  profile?: string | null;
};

/**
 * One notice for a run (D9): what it updated, every dependency those updates installed (once),
 * and why the rest failed — each reason once, with the mods it stopped (a busy profile refuses
 * every one alike). The review does not predict dependencies (D15): they are reported here.
 */
export function updatesReport(
  tr: Translate,
  attempts: readonly UpdateAttempt[],
  { single = false, profile = null }: UpdatesReportOpts = {},
): { kind: 'success' | 'warning'; title: string; lines: ReportLine[] } | null {
  if (attempts.length === 0) return null;
  const done = attempts.flatMap((a) => (a.ok ? [a] : []));
  const failed = attempts.flatMap((a) => (a.ok ? [] : [a]));
  const where = profile ? [tr('mods.ops.restore.inProfile', { profile })] : [];
  // Said only when it is known of every failure: each left its old version in place.
  const kept =
    failed.length > 0 && failed.every((a) => a.previousKept)
      ? [tr('mods.updates.previousKept')]
      : [];
  const first = failed[0];
  if (single && attempts.length === 1 && first) {
    return {
      kind: 'warning',
      title: tr('mods.updates.updateFailed', { name: first.name }),
      lines: [first.reason, ...kept, ...where],
    };
  }
  const deps = depsLines(
    tr,
    done.map((a) => a.summary),
  );
  if (failed.length === 0) {
    return {
      kind: 'success',
      title: tr('mods.installed.toastUpdated', { count: done.length }),
      lines: [...deps, ...where],
    };
  }
  const reasons = reasonLines(failed);
  return {
    kind: 'warning',
    title: tr('mods.installed.toastUpdatedFailed', { count: done.length, failed: failed.length }),
    lines: [...deps, ...reasons, ...kept, ...where],
  };
}

export function pushUpdatesReport(
  attempts: readonly UpdateAttempt[],
  opts: UpdatesReportOpts = {},
): void {
  const report = updatesReport(get(t), attempts, opts);
  if (report === null) return;
  if (report.kind === 'success') pushSuccess(report.title, report.lines);
  else pushWarning(report.title, report.lines);
}

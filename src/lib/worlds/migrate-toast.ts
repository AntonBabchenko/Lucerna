// The completion toast for a world migration — a pure builder so its honesty
// can be pinned in vitest without mounting WorldsTab (spec §7 "Completion",
// §8 "Not errors").
//
// Everything after the point of no return is an OUTCOME, not an error: the
// world IS in the target, and every line here says what else happened. A
// caveat line never invites a retry (a retry would create "World (2)") and
// names the instance a leftover sits in by its display name, so a user acting
// on the line deletes the right thing (#385).
//
// This is also where the dialog's plan-time promises meet reality: it said a
// pack already in the target library "will be linked to it" and that a move's
// backups "will move with the world". Wherever that did not happen, the line
// below says so plainly rather than leaving the promise standing.

import type { Translate } from '$lib/i18n';
import type { MigrationMode, MigrationOutcome } from '$lib/ipc/bindings';
import { isPartialOutcome } from '$lib/tasks/adapters/world-migrate';
import { leftReasonKey } from '$lib/worlds/migrate-plan-text';

export type MigrationToast = {
  /** `warning` when the outcome carries something to read — the toast then
   *  stays until dismissed, like every other outcome the user may act on. */
  kind: 'success' | 'warning';
  title: string;
  lines: string[];
};

export type MigrationToastInput = {
  mode: MigrationMode;
  outcome: MigrationOutcome;
  /** The world's folder name in the SOURCE — the name the user clicked. */
  sourceWorld: string;
  /** Display names, never ids. */
  sourceName: string;
  targetName: string;
};

export function buildMigrationToast(t: Translate, input: MigrationToastInput): MigrationToast {
  const { mode, outcome, sourceWorld, targetName } = input;
  const title =
    mode === 'move'
      ? t('worlds.migrate.toast.titleMoved', { name: sourceWorld })
      : t('worlds.migrate.toast.titleCopied', { name: sourceWorld });
  const where = t('worlds.migrate.toast.nowIn', {
    target: targetName,
    name: outcome.final_folder_name,
  });
  return {
    // ONE predicate for the task strip and the toast: `isPartialOutcome` is
    // what the adapter already finished the task with, so an amber `partial`
    // strip can never be followed by a plain green toast. It flags exactly the
    // cases `caveatLines` writes a line for — `backups_left` is the only field
    // where the two could part, and a copy reports `(0, 0)` backups by
    // construction (D4, `worlds::migrate`), so it never does in practice.
    kind: isPartialOutcome(outcome) ? 'warning' : 'success',
    title,
    lines: [where, ...caveatLines(t, input)],
  };
}

function caveatLines(t: Translate, input: MigrationToastInput): string[] {
  const { mode, outcome, sourceWorld, sourceName, targetName } = input;
  const lines: string[] = [];
  for (const d of outcome.datapacks) {
    if (d.result.kind === 'left_as_copy') {
      lines.push(
        t('worlds.migrate.toast.datapackLeftAsCopy', {
          filename: d.filename,
          reason: t(leftReasonKey(d.result.reason)),
        }),
      );
    } else if (d.result.kind === 'copied_not_linked') {
      lines.push(t('worlds.migrate.toast.datapackCopiedNotLinked', { filename: d.filename }));
    }
  }
  if (outcome.links_skipped > 0) {
    lines.push(t('worlds.migrate.toast.linksSkipped', { count: outcome.links_skipped }));
  }
  // Backups travel only on a move (D4). On a copy they stay by design and the
  // dialog already said so — a "stayed behind" line there would read as a
  // failure that did not happen.
  if (mode === 'move' && outcome.backups_left > 0) {
    lines.push(
      t('worlds.migrate.toast.backupsLeft', { count: outcome.backups_left, source: sourceName }),
    );
  }
  const state = outcome.source_state;
  if (state.kind === 'left_intact') {
    lines.push(
      t('worlds.migrate.toast.sourceLeftIntact', {
        name: sourceWorld,
        source: sourceName,
        target: targetName,
      }),
    );
  } else if (state.kind === 'left_partial') {
    lines.push(
      t('worlds.migrate.toast.sourceLeftPartial', {
        name: sourceWorld,
        source: sourceName,
        target: targetName,
      }),
    );
  }
  return lines;
}

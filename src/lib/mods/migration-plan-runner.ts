// Migration-plan runner: owns the progress Channel of `mods_plan_mc_migration`.
// The Channel lives here, not in MigrationPlanDialog, for the reason
// prefill-runner.ts and clone-runner.ts give: a component that constructs a
// Channel drags `@tauri-apps/api/core` into every test that renders it.

import { Channel } from '@tauri-apps/api/core';
import type { MigrationPlanProgress } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';

export type PlanProgressCb = (progress: MigrationPlanProgress) => void;

/** Ask for the migration plan, reporting every progress tick. Resolves with
 *  the command's own result — the dialog already handles both arms. */
export function loadMigrationPlan(instanceId: string, onProgress: PlanProgressCb) {
  const ch = new Channel<MigrationPlanProgress>();
  ch.onmessage = (p) => onProgress(p);
  return commands.modsPlanMcMigration(instanceId, ch);
}

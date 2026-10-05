// Shared controller for "apply a modpack update": fetch the new archive,
// compute the diff (for the confirm dialog), then apply through the
// `pack-update` task adapter with live progress. One instance per consuming
// surface (detail drawer, Overview card) so neither re-implements the flow.
// Runes-based factory — same shape as createQuickWorlds / createMcVersions.
//
// Only the APPLY step is task-shaped. `preparing` / `confirming` are
// interactive pre-steps whose output gates a confirm dialog, so they stay
// here; and `confirm()` keeps returning its promise because all three
// callers sequence follow-up work on it.

import type { InstanceWithStatus, ModpackUpdateDiff, ModpackVersionEntry } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { applyModpackUpdate } from '$lib/tasks/adapters/pack-update';

export type UpdateFlowPhase = 'idle' | 'preparing' | 'confirming' | 'applying';
/** What an in-flight update is doing: fetching file `current` of `total`, zipping world `current`
 *  of `total` (`fileName` is then the world's folder name), or moving the new files in, which has
 *  no count of its own. */
export type UpdateFileProgress =
  | { phase?: 'installing_file'; current: number; total: number; fileName: string }
  | { phase: 'backing_up_world'; current: number; total: number; fileName: string }
  | { phase: 'applying_changes' };
export type ConfirmOptions = { backupWorlds: boolean };

export function createModpackUpdateFlow() {
  let phase = $state<UpdateFlowPhase>('idle');
  let diff = $state<ModpackUpdateDiff | null>(null);
  let progress = $state<UpdateFileProgress | null>(null);
  let error = $state<string | null>(null);
  // How many worlds the profile has, for the «Back up worlds first» choice.
  // `null` = the count could not be read: the confirm surfaces still offer the
  // backup (not knowing is no reason to take the choice away).
  let worldCount = $state<number | null>(0);
  // Non-reactive carry-over between prepare() and confirm().
  let tempPath: string | null = null;
  let versionId: string | null = null;

  async function countWorlds(instanceId: string): Promise<number | null> {
    try {
      const r = await commands.listWorldNames(instanceId);
      return r.status === 'ok' ? r.data.length : null;
    } catch {
      // The bridge failed: the count is unknown, not zero.
      return null;
    }
  }

  // Step 1: fetch the new archive + compute the diff → open the confirm dialog.
  async function prepare(inst: InstanceWithStatus, entry: ModpackVersionEntry): Promise<void> {
    if (!inst.mrpack_project_id) return;
    error = null;
    phase = 'preparing';
    versionId = entry.id;
    const fetched = await commands.modpackFetchToTemp(
      inst.mrpack_source ?? 'modrinth',
      inst.mrpack_project_id,
      entry.id,
    );
    if (fetched.status === 'error') {
      error = formatError(fetched.error);
      phase = 'idle';
      return;
    }
    tempPath = fetched.data;
    const d = await commands.modpackComputeUpdate(inst.id, tempPath);
    if (d.status === 'error') {
      error = formatError(d.error);
      phase = 'idle';
      return;
    }
    worldCount = await countWorlds(inst.id);
    diff = d.data;
    phase = 'confirming';
  }

  // Step 2: apply. Returns true on success so the caller can refresh.
  async function confirm(
    inst: InstanceWithStatus,
    opts: ConfirmOptions = { backupWorlds: false },
  ): Promise<boolean> {
    if (!tempPath || !versionId) return false;
    // Handed over, not kept: `modpack_apply_update` deletes the staged archive
    // when it returns, whatever the outcome (and a queued apply cancelled
    // before it ran leaves it to the backend's day-old sweep), so applying
    // again means preparing again.
    const archive = tempPath;
    const targetVersionId = versionId;
    tempPath = null;
    versionId = null;
    diff = null;
    progress = null;
    error = null;
    phase = 'applying';
    // Goes through the task adapter rather than `runUpdate` directly so the
    // apply shows up in the operations strip like every other long job. The
    // callback is still ours: the three consuming surfaces render their own
    // inline progress off `flow.progress`, and registering a task must not
    // take that away from them.
    const out = await applyModpackUpdate(
      inst.name,
      inst.id,
      archive,
      targetVersionId,
      opts.backupWorlds,
      (p) => {
        if (p?.phase === 'installing_file') {
          progress = {
            current: p.current,
            total: p.total,
            fileName: p.file_name,
            phase: 'installing_file',
          };
        } else if (p?.phase === 'backing_up_world') {
          progress = {
            current: p.current,
            total: p.total,
            fileName: p.world_name,
            phase: 'backing_up_world',
          };
        } else if (p?.phase === 'applying_changes') {
          progress = { phase: 'applying_changes' };
        }
      },
    );
    phase = 'idle';
    progress = null;
    // `cancelled` means the user dropped it from the queue before it ran —
    // not a success, but not an error banner either.
    if (out.status === 'cancelled') return false;
    if (out.status === 'error') {
      error = out.message;
      return false;
    }
    return true;
  }

  function cancel(): void {
    diff = null;
    progress = null;
    error = null;
    worldCount = 0;
    tempPath = null;
    versionId = null;
    phase = 'idle';
  }

  return {
    get phase() {
      return phase;
    },
    get diff() {
      return diff;
    },
    get progress() {
      return progress;
    },
    get error() {
      return error;
    },
    get worldCount() {
      return worldCount;
    },
    get busy() {
      return phase !== 'idle';
    },
    prepare,
    confirm,
    cancel,
  };
}

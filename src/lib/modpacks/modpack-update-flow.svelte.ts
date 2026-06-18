// Shared controller for "apply a modpack update": fetch the new archive,
// compute the diff (for the confirm dialog), then apply via runUpdate with
// live progress. One instance per consuming surface (detail drawer, Overview
// card) so neither re-implements the flow. Runes-based factory — same shape
// as createQuickWorlds / createMcVersions.

import type {
  InstanceWithStatus,
  ModpackUpdateDiff,
  ModpackVersionEntry,
} from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { runUpdate } from './update-runner';

export type UpdateFlowPhase = 'idle' | 'preparing' | 'confirming' | 'applying';
export type UpdateFileProgress = { current: number; total: number; fileName: string };

export function createModpackUpdateFlow() {
  let phase = $state<UpdateFlowPhase>('idle');
  let diff = $state<ModpackUpdateDiff | null>(null);
  let progress = $state<UpdateFileProgress | null>(null);
  let error = $state<string | null>(null);
  // Non-reactive carry-over between prepare() and confirm().
  let tempPath: string | null = null;
  let versionId: string | null = null;

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
    diff = d.data;
    phase = 'confirming';
  }

  // Step 2: apply. Returns true on success so the caller can refresh.
  async function confirm(inst: InstanceWithStatus): Promise<boolean> {
    if (!tempPath || !versionId) return false;
    diff = null;
    progress = null;
    error = null;
    phase = 'applying';
    const out = await runUpdate(inst.id, tempPath, versionId, (p) => {
      if (p?.phase === 'installing_file') {
        progress = { current: p.current, total: p.total, fileName: p.file_name };
      }
    });
    phase = 'idle';
    progress = null;
    if (out.status === 'error') {
      error = out.message;
      return false;
    }
    tempPath = null;
    versionId = null;
    return true;
  }

  function cancel(): void {
    diff = null;
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
    get busy() {
      return phase !== 'idle';
    },
    prepare,
    confirm,
    cancel,
  };
}

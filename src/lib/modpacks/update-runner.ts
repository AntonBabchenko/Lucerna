// Owns the apply IPC + its two progress Channels, lifted out of the Svelte
// component so the channel wiring is unit-testable. Direct mirror of
// src/lib/ops/import-runner.ts (runImport) — including its rule that the
// channels carry PROGRESS ONLY and every result field comes off `r.data`.

import { Channel } from '@tauri-apps/api/core';
import type {
  InertLoaderJar,
  InstanceWithStatus,
  ModpackProgress,
  ProgressTick,
  TaskDetail,
} from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

export type UpdateProgressCb = (phase: ModpackProgress | null, bytes: ProgressTick | null) => void;

export type UpdateOutcome =
  | {
      status: 'ok';
      inst: InstanceWithStatus;
      inertLoaderJars: InertLoaderJar[];
      /** Per-file rows for the removals + installs this update performed —
       *  what the task registry hands to the install-report modal. */
      details: TaskDetail[];
    }
  | { status: 'error'; message: string };

export async function runUpdate(
  instanceId: string,
  tempPath: string,
  newVersionId: string,
  onProgress: UpdateProgressCb,
): Promise<UpdateOutcome> {
  let latestPhase: ModpackProgress | null = null;
  let latestBytes: ProgressTick | null = null;
  onProgress(null, null);

  const phaseChannel = new Channel<ModpackProgress>();
  phaseChannel.onmessage = (m) => {
    latestPhase = m;
    onProgress(latestPhase, latestBytes);
  };
  const tickChannel = new Channel<ProgressTick>();
  tickChannel.onmessage = (tk) => {
    latestBytes = tk;
    onProgress(latestPhase, latestBytes);
  };

  const r = await commands.modpackApplyUpdate(
    instanceId,
    tempPath,
    newVersionId,
    phaseChannel,
    tickChannel,
  );
  if (r.status === 'ok') {
    return {
      status: 'ok',
      inst: r.data.instance,
      inertLoaderJars: r.data.inert_loader_jars,
      details: r.data.details,
    };
  }
  return { status: 'error', message: formatError(r.error) };
}

// Owns the apply IPC + its two progress Channels, lifted out of the Svelte
// component so the channel wiring is unit-testable. Direct mirror of
// src/lib/ops/import-runner.ts (runImport).

import { Channel } from '@tauri-apps/api/core';
import type { InstanceWithStatus, ModpackProgress, ProgressTick } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

export type UpdateProgressCb = (phase: ModpackProgress | null, bytes: ProgressTick | null) => void;

export type UpdateOutcome =
  | { status: 'ok'; inst: InstanceWithStatus }
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
  if (r.status === 'ok') return { status: 'ok', inst: r.data };
  return { status: 'error', message: formatError(r.error) };
}

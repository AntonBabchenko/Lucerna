// The modpack-import runner, lifted out of +page.svelte's runModpackImport.
// Owns the two progress Channels + the modpackImport command; returns a
// structured outcome. Toasting + navigation belong to the op-queue store (it
// owns completionTick), so this stays pure (and unit-testable).
//
// The channels carry PROGRESS ONLY. Everything the outcome needs comes off
// `r.data`, because a Channel message and the command's response travel by
// different transports: Tauri routes any channel payload at or above 8192
// bytes through a second async IPC round trip (`ipc/channel.rs`), which lands
// after the response has already resolved. Reading a `done` message into a
// local and returning it right after the `await` therefore reported "0 files"
// for every pack big enough to matter — a 37-file report is ~10 KB.

import { Channel } from '@tauri-apps/api/core';
import type {
  InertLoaderJar,
  ModpackProgress,
  ProgressTick,
  SkippedOverride,
  TaskDetail,
} from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import type { ModpackImportRequest } from '$lib/modpacks/import-request';

export type ImportProgressCb = (phase: ModpackProgress | null, bytes: ProgressTick | null) => void;

export type ImportOutcome =
  | {
      status: 'ok';
      name: string;
      instanceId: string;
      skipped: SkippedOverride[];
      inertLoaderJars: InertLoaderJar[];
      details: TaskDetail[];
    }
  | { status: 'partial'; failed: string[] }
  | { status: 'error'; message: string };

export async function runImport(
  req: ModpackImportRequest,
  onProgress: ImportProgressCb,
): Promise<ImportOutcome> {
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

  const r = await commands.modpackImport(
    req.path,
    req.selectedShas,
    true,
    req.projectId,
    req.source,
    req.versionId,
    phaseChannel,
    tickChannel,
  );

  if (r.status === 'ok') {
    return {
      status: 'ok',
      name: r.data.instance.name,
      instanceId: r.data.instance.id,
      skipped: r.data.skipped_overrides,
      inertLoaderJars: r.data.inert_loader_jars,
      details: r.data.details,
    };
  }
  if (r.error.kind === 'modpack_partial_failure') {
    return { status: 'partial', failed: r.error.failed.map(([p]) => p.split('/').pop() ?? p) };
  }
  return { status: 'error', message: formatError(r.error) };
}

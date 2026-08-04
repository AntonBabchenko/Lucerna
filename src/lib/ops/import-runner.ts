// The modpack-import runner, lifted out of +page.svelte's runModpackImport.
// Owns the two progress Channels + the modpackImport command; returns a
// structured outcome. Toasting + navigation belong to the op-queue store (it
// owns completionTick), so this stays pure (and unit-testable).

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
  let skipped: SkippedOverride[] = [];
  let inertLoaderJars: InertLoaderJar[] = [];
  let details: TaskDetail[] = [];
  onProgress(null, null);

  const phaseChannel = new Channel<ModpackProgress>();
  phaseChannel.onmessage = (m) => {
    latestPhase = m;
    if (m.phase === 'done') {
      skipped = m.skipped_overrides;
      inertLoaderJars = m.inert_loader_jars;
      details = m.details;
    }
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
      name: r.data.name,
      instanceId: r.data.id,
      skipped,
      inertLoaderJars,
      details,
    };
  }
  if (r.error.kind === 'modpack_partial_failure') {
    return { status: 'partial', failed: r.error.failed.map(([p]) => p.split('/').pop() ?? p) };
  }
  return { status: 'error', message: formatError(r.error) };
}

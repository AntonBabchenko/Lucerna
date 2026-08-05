// Launcher-instance import runner: wraps the single-channel launcherImportRun
// command and returns a structured outcome. Toasting + navigation belong to the
// op-queue store (it owns completionTick), so this stays pure and unit-testable.
//
// The channel carries PROGRESS ONLY — see the same note in
// `$lib/ops/import-runner`. `untracked_mods` is a result, so it comes off
// `r.data`, not out of a captured `done` message.

import { Channel } from '@tauri-apps/api/core';
import type { ContentCategory, ForeignInstance, ImportProgress } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

export type LauncherImportRequest = {
  foreign: ForeignInstance;
  selected: ContentCategory[];
  targetName: string;
  mcVersionOverride?: string | null;
  loaderOverride?: 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;
  loaderVersionOverride?: string | null;
};

export type LauncherImportProgressCb = (phase: ImportProgress | null) => void;

export type LauncherImportOutcome =
  | { status: 'ok'; name: string; instanceId: string; untrackedMods: number }
  | { status: 'error'; message: string };

export async function runLauncherImport(
  req: LauncherImportRequest,
  onProgress: LauncherImportProgressCb,
): Promise<LauncherImportOutcome> {
  let latestPhase: ImportProgress | null = null;
  onProgress(null);

  const ch = new Channel<ImportProgress>();
  ch.onmessage = (m) => {
    latestPhase = m;
    onProgress(latestPhase);
  };

  const r = await commands.launcherImportRun(
    req.foreign,
    req.selected,
    req.targetName,
    req.mcVersionOverride ?? null,
    req.loaderOverride ?? null,
    req.loaderVersionOverride ?? null,
    ch,
  );

  if (r.status === 'ok') {
    return {
      status: 'ok',
      name: r.data.instance.name,
      instanceId: r.data.instance.id,
      untrackedMods: r.data.untracked_mods,
    };
  }
  return { status: 'error', message: formatError(r.error) };
}

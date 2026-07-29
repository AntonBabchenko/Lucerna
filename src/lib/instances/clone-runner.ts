// Clone runner: wraps the single-channel cloneInstance command and returns a
// structured outcome. Toasting + list refresh belong to the op-queue store
// (it owns completionTick), so this stays pure and unit-testable. Mirrors
// launcher-import-runner.ts.

import { Channel } from '@tauri-apps/api/core';
import type { CloneProgress } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import type { CloneRequest } from './clone-request';

export type CloneProgressCb = (phase: CloneProgress | null) => void;

export type CloneOutcome =
  | { status: 'ok'; instanceId: string; name: string }
  | { status: 'error'; message: string };

export async function runClone(
  req: CloneRequest,
  onProgress: CloneProgressCb,
): Promise<CloneOutcome> {
  onProgress(null);

  const ch = new Channel<CloneProgress>();
  ch.onmessage = (m) => onProgress(m);

  const r = await commands.cloneInstance(req.sourceId, req.newName, req.options, ch);
  if (r.status === 'ok') {
    return { status: 'ok', instanceId: r.data.id, name: r.data.name };
  }
  return { status: 'error', message: formatError(r.error) };
}

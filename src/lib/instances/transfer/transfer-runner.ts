// Runs one mod-data copy (spec 2026-10-15 Part A): the command and its progress
// channel. The task adapter (`$lib/tasks/adapters/transfer`) owns the task row.
import { Channel } from '@tauri-apps/api/core';
import type {
  OnConflict,
  TransferOutcome,
  TransferProgress,
  TransferSelection,
} from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

/** One queued copy: both instances (names for the toast) and the dialog's choices. */
export interface TransferRequest {
  sourceId: string;
  sourceName: string;
  targetId: string;
  targetName: string;
  selection: TransferSelection;
  onConflict: OnConflict;
}

export type TransferRunOutcome =
  | { status: 'ok'; outcome: TransferOutcome }
  | { status: 'error'; message: string };

export async function runTransfer(
  req: TransferRequest,
  onProgress: (p: TransferProgress | null) => void,
): Promise<TransferRunOutcome> {
  onProgress(null);
  const ch = new Channel<TransferProgress>();
  ch.onmessage = (m) => onProgress(m);
  const r = await commands.transferModData(
    req.sourceId,
    req.targetId,
    req.selection,
    req.onConflict,
    ch,
  );
  if (r.status === 'ok') return { status: 'ok', outcome: r.data };
  return { status: 'error', message: formatError(r.error) };
}

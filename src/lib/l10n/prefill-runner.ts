// Pre-fill runner: wraps the single-channel l10nPrefillStart command and
// returns a structured outcome. Mirrors instances/clone-runner.ts.
//
// The `Channel` construction lives here rather than in PrefillDialog for the
// same reason it lives in clone-runner: a component that news up a Channel
// drags `@tauri-apps/api/core` into every test that renders it, and the
// progress ticks become unreachable from a test. With the channel here, the
// dialog only ever sees a callback.

import { Channel } from '@tauri-apps/api/core';
import { commands, type PrefillProgress, type RunSummary } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

export type PrefillProgressCb = (progress: PrefillProgress | null) => void;

export type PrefillOutcome =
  | { status: 'ok'; summary: RunSummary }
  | { status: 'error'; message: string };

/** Start a run and resolve once it finishes. `namespace` scopes the run to one
 *  resource namespace; `null` covers the whole instance.
 *
 *  A run that stopped part-way still resolves `ok` — the failure is reported
 *  ON the summary (`RunSummary.failed`), because everything before it was
 *  verified, paid for and flushed. Only a call that never produced a summary
 *  at all (busy, consent off, no API key) is an `error` here. */
export async function runPrefill(
  instanceId: string,
  lang: string,
  namespace: string | null,
  onProgress: PrefillProgressCb,
): Promise<PrefillOutcome> {
  onProgress(null);

  const ch = new Channel<PrefillProgress>();
  ch.onmessage = (p) => onProgress(p);

  const r = await commands.l10nPrefillStart(instanceId, lang, namespace, ch);
  if (r.status === 'ok') return { status: 'ok', summary: r.data };
  return { status: 'error', message: formatError(r.error) };
}

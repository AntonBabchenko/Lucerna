// Module-singleton rune store for the active instance's latest-log diagnosis.
// Same idiom as repair-ops.svelte / op-queue.svelte: module-level $state read in
// templates via exported getters. The page wires WHEN to refresh (instance
// change / processExited / repair completion); this module only fetches + holds.

import { commands, type DiagnosisStatus, type LatestDiagnosis } from '$lib/ipc/bindings';

let current = $state<LatestDiagnosis | null>(null);
let forInstance = $state<string | null>(null);
let seq = 0;

/** Fetch the latest diagnosis for `instanceId`, or clear when null. */
export async function refreshDiagnosis(instanceId: string | null): Promise<void> {
  if (!instanceId) {
    // Bump seq so an in-flight fetch for a previous instance can't write its
    // (now-stale) result back over the cleared state once it resolves.
    seq++;
    current = null;
    forInstance = null;
    return;
  }
  const ticket = ++seq;
  forInstance = instanceId;
  const res = await commands.diagnoseLatest(instanceId);
  // Drop stale responses (instance switched mid-flight).
  if (ticket !== seq) return;
  current = res.status === 'ok' ? res.data : null;
}

export function latestDiagnosis(): LatestDiagnosis | null {
  return current;
}

export function diagnosisStatus(): DiagnosisStatus {
  return current?.status ?? 'none';
}

/** True when there is an unresolved problem worth a badge/indicator. */
export function hasDiagnosisIndicator(): boolean {
  const s = current?.status;
  return s === 'actionable' || s === 'advisory';
}

/** Which instance the current state belongs to (useful for the page). */
export function diagnosisForInstance(): string | null {
  return forInstance;
}

/** Test-only reset of the module singleton. */
export function __resetLogDiagnosisForTest(): void {
  current = null;
  forInstance = null;
  seq = 0;
}

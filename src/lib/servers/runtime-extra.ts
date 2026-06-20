// Contract shim for the C1 cross-stream fields (S1 → S3, see
// docs/superpowers/plans/2026-06-20-servers-parallel-coordination.md):
//
//   pub diagnosis_status: ServerDiagnosisStatus,  // 'none' | 'advisory' | 'actionable'
//   pub last_exit_code: Option<i32>,              // None = clean/never-run, Some(n) = crashed
//
// S3 lands last, so these fields aren't on the generated `ServerWithStatus` yet.
// We read them through this narrow, documented shim so the frontend can be coded
// against the agreed shape now. AFTER `git merge origin/main` regenerates the
// bindings, the casts below collapse to plain field reads — keep this module so
// the call sites stay stable; just drop the `as` once the fields are real.

import type { ServerWithStatus } from '$lib/ipc/bindings';

// Matches S1's `DiagnosisStatus` wire shape (lowercase). 'handled' = a fix was
// already applied for the current log, so it is NOT actionable.
export type ServerDiagnosisStatusWire = 'none' | 'advisory' | 'actionable' | 'handled';

/** The server's latest diagnosis severity ('none' until S1's field lands). */
export function diagnosisStatusOf(s: ServerWithStatus): ServerDiagnosisStatusWire {
  return (s as { diagnosis_status?: ServerDiagnosisStatusWire }).diagnosis_status ?? 'none';
}

/** True when a one-click repair is available for this server. */
export function isDiagnosisActionable(s: ServerWithStatus): boolean {
  return diagnosisStatusOf(s) === 'actionable';
}

/** The last process exit code, or null for a clean exit / never run. */
export function lastExitCodeOf(s: ServerWithStatus): number | null {
  return (s as { last_exit_code?: number | null }).last_exit_code ?? null;
}

/**
 * Whether the server's last run crashed: it's stopped and the last exit code is
 * a non-zero/non-clean value. A user-requested Stop reports a clean exit on the
 * backend, so this stays false for normal shutdowns.
 */
export function isCrashed(s: ServerWithStatus): boolean {
  if (s.running) return false;
  const code = lastExitCodeOf(s);
  return code !== null && code !== 0;
}

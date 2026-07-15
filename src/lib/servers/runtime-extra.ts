// Small read helpers over a server's runtime status fields (C1 contract from
// S1: `diagnosis_status` + `last_exit_code`, now present in the generated
// bindings). Kept as a module so the call sites (sidebar badge, attention item,
// crash pill) share one definition of "actionable" / "crashed".

import type { DiagnosisStatus, ServerWithStatus } from '$lib/ipc/bindings';

/** The server's latest diagnosis severity. */
export function diagnosisStatusOf(s: ServerWithStatus): DiagnosisStatus {
  return s.diagnosis_status;
}

/** True when a one-click repair is available for this server. */
export function isDiagnosisActionable(s: ServerWithStatus): boolean {
  return s.diagnosis_status === 'actionable';
}

/** The last process exit code, or null for a clean exit / never run. */
export function lastExitCodeOf(s: ServerWithStatus): number | null {
  return s.last_exit_code;
}

/** Force-kill sentinel the backend records when it terminates a server itself
 * (a graceful Stop that timed out — common when stopping a server that is still
 * loading). Mirrors the backend `is_crash_exit`: it is a stop, not a crash. */
const FORCE_KILL_EXIT = -1;

/**
 * Whether the server's last run crashed: it's stopped and the last exit code is
 * a real crash code. A user-requested Stop reports either a clean exit (0) or the
 * force-kill sentinel (-1) on the backend — both are stops, not crashes — so this
 * stays false for normal shutdowns, including a Stop issued while the server was
 * still loading. Kept in sync with the backend `is_crash_exit`.
 */
export function isCrashed(s: ServerWithStatus): boolean {
  if (s.running) return false;
  return (
    s.last_exit_code !== null && s.last_exit_code !== 0 && s.last_exit_code !== FORCE_KILL_EXIT
  );
}

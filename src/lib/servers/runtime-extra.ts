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

/**
 * Whether the server's last run crashed: it's stopped and the last exit code is
 * a non-zero/non-clean value. A user-requested Stop reports a clean exit on the
 * backend, so this stays false for normal shutdowns.
 */
export function isCrashed(s: ServerWithStatus): boolean {
  if (s.running) return false;
  return s.last_exit_code !== null && s.last_exit_code !== 0;
}

import type { IntegrityStatus } from '$lib/ipc/bindings';

// Captured once when this module is first imported — i.e. at launcher startup,
// since the webview reloads on each launch and re-initialises module state.
// This is the "this session" boundary for integrity freshness.
const SESSION_START_MS = Date.now();

/**
 * A *healthy* integrity result is only meaningful for the session in which it
 * was produced: a persisted "✓ OK" from a previous launcher run gives false
 * confidence because the instance may have changed since (the result is never
 * invalidated on the backend). So we session-scope healthy results — a healthy
 * check from before this session counts as stale and should render as
 * "not checked".
 *
 * Problem results (⚠ N) are deliberately *not* stale: they are an actionable
 * reminder that a repair is still pending, valid until the user re-verifies.
 *
 * `sessionStartMs` is injectable for deterministic tests; production callers use
 * the module default.
 */
export function isIntegrityStale(
  status: IntegrityStatus | null | undefined,
  sessionStartMs: number = SESSION_START_MS,
): boolean {
  if (!status?.healthy) return false;
  if (status.checked_unix_ms === null) return false;
  return status.checked_unix_ms < sessionStartMs;
}

/**
 * The status a view should render: `null` (i.e. "not checked") when the result
 * is a stale healthy one, otherwise the status unchanged. Lets a template treat
 * a previous-session "OK" exactly like never-checked without special-casing.
 */
export function effectiveIntegrityStatus(
  status: IntegrityStatus | null | undefined,
  sessionStartMs: number = SESSION_START_MS,
): IntegrityStatus | null {
  if (!status) return null;
  return isIntegrityStale(status, sessionStartMs) ? null : status;
}

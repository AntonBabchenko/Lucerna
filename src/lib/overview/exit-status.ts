/**
 * Classifies how a Minecraft process run ended, so the Overview can show an
 * honest status instead of a raw OS exit code.
 *
 * The launcher force-kills the process tree when the user presses Stop, which
 * on Windows yields a non-zero exit code (commonly 1) and on Unix a
 * signal-termination (surfaced as -1). Without the `user_requested` intent from
 * the backend, those are indistinguishable from a real crash — see
 * `ProcessExited` in `src-tauri/src/launch/spawn.rs`.
 */
export type ExitStatus =
  | { kind: 'stopped' }
  | { kind: 'clean' }
  | { kind: 'crashed'; code: number };

export function classifyExit(exited: { code: number; user_requested: boolean }): ExitStatus {
  if (exited.user_requested) return { kind: 'stopped' };
  if (exited.code === 0) return { kind: 'clean' };
  return { kind: 'crashed', code: exited.code };
}

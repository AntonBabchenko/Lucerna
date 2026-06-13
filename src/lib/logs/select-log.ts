import type { LogFileMeta } from '$lib/ipc/bindings';

/**
 * Choose which log to show when the log viewer opens.
 *
 * `files` is the list from `list_log_files`, which the backend already sorts by
 * mtime descending (newest first), so `files[0]` is the newest log.
 *
 * Precedence:
 *  - an explicit `initialPath` deep-link (e.g. "view this crash log") wins, but
 *    only while that file is still present in the list (it may have rotated away);
 *  - otherwise the newest log;
 *  - `null` when there are no logs.
 *
 * Selecting the newest on every open is what refreshes a stale prior selection
 * after a new run; loading the chosen path also re-reads `latest.log`, whose
 * content changes between runs even though its path does not.
 */
export function chooseOpenLog(
  files: readonly LogFileMeta[],
  initialPath: string | null,
): string | null {
  if (initialPath && files.some((f) => f.path === initialPath)) {
    return initialPath;
  }
  return files[0]?.path ?? null;
}

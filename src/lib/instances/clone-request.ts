import type { CloneOptions } from '$lib/ipc/bindings';

/** Mirrors MAX_INSTANCE_NAME_LEN in src-tauri/src/commands/mod.rs — the
 *  backend is the source of truth and rejects longer names. */
export const INSTANCE_NAME_MAX = 32;

/** One queued clone: the source instance plus the user's dialog choices. */
export interface CloneRequest {
  sourceId: string;
  newName: string;
  options: CloneOptions;
}

/** Default display name for a clone: the source name plus the localized
 *  suffix (e.g. " (copy)"), with the base truncated so the result stays
 *  within the 32-char instance-name limit. */
export function defaultCloneName(sourceName: string, suffix: string): string {
  const room = Math.max(0, INSTANCE_NAME_MAX - suffix.length);
  const base = sourceName.length > room ? sourceName.slice(0, room).trimEnd() : sourceName;
  return `${base}${suffix}`.slice(0, INSTANCE_NAME_MAX);
}

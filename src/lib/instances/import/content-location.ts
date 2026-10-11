import type { ContentEntry } from '$lib/ipc/bindings';

/**
 * Whether a content entry can be copied: a real folder, or one reached
 * through a link (its target is copied as real files). A broken link or an
 * unreadable folder is shown but cannot be selected — the backend refuses it.
 */
export function isSelectable(entry: ContentEntry): boolean {
  return entry.location.kind === 'local' || entry.location.kind === 'linked';
}

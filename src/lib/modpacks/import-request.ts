import type { ModSource } from '$lib/ipc/bindings';

// Payload that ModpacksTab hands up to the page when the user confirms a
// modpack import from the picker. The PAGE owns the actual import execution
// (via the op-queue store, which now enqueues onto the task registry through
// `$lib/tasks/adapters/pack-import`) so that the modpacks modal can be freely
// closed mid-import — the OperationsBar strip lives at the page level and
// survives the modal (and ModpacksTab) unmounting.
export type ModpackImportRequest = {
  // Absolute path to the .mrpack/.zip the picker inspected.
  path: string;
  // SHA1s of the optional files the user chose to include.
  selectedShas: string[];
  // The pack's own name, read from its manifest by the picker. Carried here
  // because the page — which titles the import task — never sees the
  // `ModpackSummary` the picker inspected, and had nothing better than the
  // platform project id to fall back on.
  displayName: string;
  // Browse-flow hints stamped onto the new instance (null for drag-drop).
  projectId: string | null;
  source: ModSource | null;
  versionId: string | null;
};

/** Title for a pack-import task. The pack's own name when the picker read
 *  one, else the archive filename. Deliberately never `projectId`: that is an
 *  opaque platform code (`1KVo5zza`) the user has never seen, and it used to
 *  be the FIRST choice — which is the bug this replaces. */
export function importTitle(request: ModpackImportRequest): string {
  const name = request.displayName.trim();
  if (name) return name;
  const file = request.path.split(/[\\/]/).pop()?.trim();
  return file || 'modpack';
}

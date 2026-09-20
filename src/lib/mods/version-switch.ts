// «Is this row the build that is installed?» and «does installing this build
// replace another one?» — asked by the detail modal and by every view that
// installs from it. Pure; type-only import.
//
// RED-ROUND SCAFFOLD: today's rule — a build is recognised by its version id
// only. Task G5 adds recognition by bytes (2026-09-20 spec, D7).
import type { ModVersion } from '$lib/ipc/bindings';

/** The registry's view of an installed build, as far as a switch needs it. */
export type InstalledBuild = { sha1: string; version_id: string | null };

export function isInstalledBuild(
  v: ModVersion,
  installedVersionId: string | null,
  _installedSha1: string | null,
): boolean {
  return installedVersionId !== null && v.version_id === installedVersionId;
}

/** The sha1 `mods_update_one` must replace when installing `v` is a version
 *  switch; `null` when it is a fresh install. */
export function switchTarget(existing: InstalledBuild | null, v: ModVersion): string | null {
  if (!existing) return null;
  return existing.version_id !== v.version_id ? existing.sha1 : null;
}

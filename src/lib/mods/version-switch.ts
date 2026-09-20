// «Is this row the build that is installed?» and «does installing this build
// replace another one?» — asked by the detail modal and by every view that
// installs from it. Pure; type-only import.
import type { ModVersion } from '$lib/ipc/bindings';

/** The registry's view of an installed build, as far as a switch needs it. */
export type InstalledBuild = { sha1: string; version_id: string | null };

/** D7 — the installed build is recognised by its version id OR by its bytes.
 *  `enrich` leaves the registry's `version_id` null for exactly the mods whose
 *  platform tags disagree with the instance; by id alone such a build is never
 *  «the installed one», and every click on it looked like a version switch.
 *  An absent or empty digest matches nothing: two unknowns are not the same. */
export function isInstalledBuild(
  v: ModVersion,
  installedVersionId: string | null,
  installedSha1: string | null,
): boolean {
  if (installedVersionId !== null && v.version_id === installedVersionId) return true;
  const fileSha1 = v.primary_file.sha1;
  if (!fileSha1 || !installedSha1) return false;
  return fileSha1.toLowerCase() === installedSha1.toLowerCase();
}

/** The sha1 `mods_update_one` must replace when installing `v` is a version
 *  switch; `null` when it is a fresh install — nothing of this project is
 *  installed, or the picked build IS the installed one. */
export function switchTarget(existing: InstalledBuild | null, v: ModVersion): string | null {
  if (!existing) return null;
  return isInstalledBuild(v, existing.version_id, existing.sha1) ? null : existing.sha1;
}

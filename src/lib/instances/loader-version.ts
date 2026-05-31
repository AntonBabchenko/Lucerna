import type { LoaderVersion } from '$lib/ipc/bindings';

/**
 * Decide which loader version a (loader, mc) pair should resolve to.
 *
 * - `resetToStable` (the user just switched loader ecosystems) → always the
 *   recommended/stable build, ignoring any carried-over version number.
 * - otherwise (mount / MC change) → keep the stored version if it is still a
 *   real build for this MC, else fall back to the recommended one.
 *
 * Returns `null` only when the platform offers no builds at all.
 *
 * The caller commits the result whenever it differs from what was stored, so
 * the picker's display can never diverge from the instance's saved version
 * (a stale saved version was the cause of the Forge-install 404).
 */
export function resolveLoaderVersion(
  stored: string | null,
  list: LoaderVersion[],
  resetToStable: boolean,
): string | null {
  const fallback = (list.find((l) => l.stable) ?? list[0])?.version ?? null;
  if (resetToStable) return fallback;
  const storedIsValid = stored != null && list.some((l) => l.version === stored);
  return storedIsValid ? stored : fallback;
}

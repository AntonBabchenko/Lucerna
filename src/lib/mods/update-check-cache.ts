import type { ModUpdateCheck } from '$lib/ipc/bindings';

// Per-instance, process-lifetime cache of the last mod-update-check
// result. Keyed by instance id; the value is that instance's list of
// per-mod check results (one ModUpdateCheck per eligible mod). A check
// fires one platform request per installed user-mod, so the result is
// kept for the app session: reopening the Installed tab does not
// re-check — the "Check for updates" button forces a fresh check.
// Module-scoped, never persisted to disk.
export const updateCheckCache = new Map<string, ModUpdateCheck[]>();

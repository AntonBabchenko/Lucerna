import type { ModUpdateCheck } from '$lib/ipc/bindings';
import { createLru } from './lru';

// Per-instance, process-lifetime LRU cache (cap 5) of the last mod-update-check
// result (one ModUpdateCheck per eligible mod). A check fires one platform
// request per installed user-mod, so the result is kept for the session;
// reopening the Installed tab does not re-check. Bounded LRU, never persisted.
export const updateCheckCache = createLru<ModUpdateCheck[]>(5);

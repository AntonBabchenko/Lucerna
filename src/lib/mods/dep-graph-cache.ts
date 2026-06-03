import type { DependencyGraph } from '$lib/ipc/bindings';
import { createLru } from './lru';

// Per-instance, process-lifetime LRU cache (cap 5) of the last dependency
// graph result. Never persisted to disk; bounded so navigating many instances
// over a session does not grow unbounded.
export const depGraphCache = createLru<DependencyGraph>(5);

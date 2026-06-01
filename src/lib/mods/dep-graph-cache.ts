import type { DependencyGraph } from '$lib/ipc/bindings';

// Per-instance, process-lifetime cache of the last dependency graph
// result. Keyed by instance id. Mirrors update-check-cache.ts's
// module-singleton style — never persisted to disk.
const cache = new Map<string, DependencyGraph>();

export const depGraphCache = {
  get: (id: string) => cache.get(id),
  set: (id: string, g: DependencyGraph) => cache.set(id, g),
  delete: (id: string) => cache.delete(id),
};

// Minimal insertion/access-ordered LRU over a Map. `get` promotes the key to
// most-recently-used; `set` past the cap evicts the least-recently-used. Used
// for the per-session dependency-graph and update-check caches.
export function createLru<V>(cap: number) {
  const map = new Map<string, V>();
  return {
    get(id: string): V | undefined {
      if (!map.has(id)) return undefined;
      const v = map.get(id) as V;
      map.delete(id);
      map.set(id, v); // re-insert → most recently used
      return v;
    },
    set(id: string, value: V): void {
      if (map.has(id)) map.delete(id);
      map.set(id, value);
      while (map.size > cap) {
        const oldest = map.keys().next().value as string | undefined;
        if (oldest === undefined) break;
        map.delete(oldest);
      }
    },
    delete(id: string): void {
      map.delete(id);
    },
  };
}

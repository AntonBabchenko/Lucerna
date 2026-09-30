/**
 * Writes under way, by key — the reentrancy gate of the mod row actions. A row action has no busy
 * state of its own, so a double click on Remove or «Don't update» used to fire the same write
 * twice. A request whose key is already held is ignored by its caller, silently: the write that
 * holds it is still running and reports for both.
 *
 * Each claim releases only the keys it took, so `clear()` (tests) never lets a late release free a
 * newer claim of the same key.
 */
export type InFlightClaim = {
  /** The keys this claim took — those no other claim held. */
  readonly free: ReadonlySet<string>;
  /** Give them back; call it in `finally`, whatever the write did. */
  release(): void;
};

export type InFlight = {
  /** Take every key of `keys` that no claim holds. */
  claim(keys: Iterable<string>): InFlightClaim;
  clear(): void;
};

export function createInFlight(): InFlight {
  const held = new Map<string, symbol>();
  return {
    claim(keys) {
      const owner = Symbol('claim');
      const free = new Set<string>();
      for (const key of keys) {
        if (held.has(key)) continue;
        held.set(key, owner);
        free.add(key);
      }
      return {
        free,
        release() {
          for (const key of free) if (held.get(key) === owner) held.delete(key);
        },
      };
    },
    clear() {
      held.clear();
    },
  };
}

import { commands } from '$lib/ipc/bindings';

// Module-scope cache keyed by account UUID. Stores the in-flight/settled
// promise so concurrent component mounts and Select re-renders share one IPC
// call. Values are the full skin PNG base64, or null when there is no skin
// (the component then renders the letter fallback).
//
// This is a PER-SESSION dedup layer only. The real refresh mechanism is the
// Rust on-disk cache (6h TTL) plus the sign-in/refresh skin prefetch. A
// settled value (base64 or null) is kept for the session; a REJECTED IPC call
// is evicted so a later render can retry after a transient bridge failure.
const cache = new Map<string, Promise<string | null>>();

export function loadSkinHead(uuid: string): Promise<string | null> {
  let pending = cache.get(uuid);
  if (!pending) {
    pending = commands
      .accountSkin(uuid)
      .then((res) => (res.status === 'ok' ? (res.data?.skin_png_base64 ?? null) : null))
      .catch(() => {
        // Transient IPC failure (bridge not ready, etc.) — drop the entry so
        // the next render retries rather than showing the fallback all session.
        cache.delete(uuid);
        return null;
      });
    cache.set(uuid, pending);
  }
  return pending;
}

/** Test-only: reset the module cache between cases. */
export function __clearSkinCache(): void {
  cache.clear();
}

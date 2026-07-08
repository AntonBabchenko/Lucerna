import { commands } from '$lib/ipc/bindings';

// Per-session dedup of instance-icon IPC loads, keyed by instance id. Mirrors
// accounts/skin-cache.ts. Values are a `data:image/png;base64,...` URL, or null
// when the instance has no custom picture (the avatar renders the letter
// fallback). A rejected IPC call is evicted so a later render can retry.
const cache = new Map<string, Promise<string | null>>();

export function loadInstanceIcon(id: string): Promise<string | null> {
  let pending = cache.get(id);
  if (!pending) {
    pending = commands
      .instanceIcon(id)
      .then((res) => {
        if (res.status !== 'ok') return null;
        const b64 = res.data?.png_base64;
        return b64 ? `data:image/png;base64,${b64}` : null;
      })
      .catch(() => {
        cache.delete(id);
        return null;
      });
    cache.set(id, pending);
  }
  return pending;
}

/** Drop a cached entry so the next load re-fetches (after set/clear). */
export function invalidateInstanceIcon(id: string): void {
  cache.delete(id);
}

/** Test-only: reset the module cache between cases. */
export function __clearInstanceIconCache(): void {
  cache.clear();
}

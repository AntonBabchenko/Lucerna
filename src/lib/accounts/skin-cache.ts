import { commands } from '$lib/ipc/bindings';

// Module-scope cache keyed by account UUID. Stores the in-flight/settled
// promise so concurrent component mounts and Select re-renders share one
// IPC call. Values are the full skin PNG base64, or null when there is no
// skin (the component then renders the letter fallback).
const cache = new Map<string, Promise<string | null>>();

export function loadSkinHead(uuid: string): Promise<string | null> {
  let pending = cache.get(uuid);
  if (!pending) {
    pending = commands
      .accountSkin(uuid)
      .then((res) => (res.status === 'ok' ? (res.data?.skin_png_base64 ?? null) : null))
      .catch(() => null);
    cache.set(uuid, pending);
  }
  return pending;
}

/** Test-only: reset the module cache between cases. */
export function __clearSkinCache(): void {
  cache.clear();
}

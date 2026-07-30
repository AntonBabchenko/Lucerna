/**
 * Adaptive heap-slider bounds, shared by every memory slider (instance detail,
 * server general/create/import). Centralizes the static fallback and the
 * one-shot fetch so the control behaves identically everywhere.
 */

import { commands, type MemoryBounds } from '$lib/ipc/bindings';

/**
 * Static fallback used until the adaptive bounds resolve, or if the query
 * fails. Mirrors the historical fixed slider (1–8 GB) so the control is never
 * broken. `ram_known: false` suppresses the high-memory warning band when the
 * real ceiling is unknown.
 */
export const FALLBACK_MEMORY_BOUNDS: MemoryBounds = {
  min_mb: 1024,
  max_mb: 8192,
  // Matches what the backend's default_heap_mb(None) returns, so a create form
  // seeded from the fallback picks the same heap the backend would have.
  default_mb: 2048,
  recommended_max_mb: 8192,
  step_mb: 256,
  ram_known: false,
};

// Physical RAM doesn't change at runtime, so a successful result is cached for
// the session. Failures are NOT cached — a transient error must not pin every
// later surface to the 8 GB fallback; the next caller re-tries. Concurrent
// callers share the in-flight promise.
let cached: MemoryBounds | null = null;
let inflight: Promise<MemoryBounds> | null = null;

/**
 * Resolve this machine's adaptive heap bounds. Degrades gracefully to
 * {@link FALLBACK_MEMORY_BOUNDS} on any error, so callers never handle failure —
 * the slider stays usable regardless.
 */
export async function loadMemoryBounds(): Promise<MemoryBounds> {
  if (cached) return cached;
  if (!inflight) {
    inflight = commands
      .instanceMemoryBounds()
      .then((bounds) => {
        cached = bounds;
        return bounds;
      })
      .catch(() => FALLBACK_MEMORY_BOUNDS)
      .finally(() => {
        inflight = null;
      });
  }
  return inflight;
}

/** Test-only: drop the cached bounds so the next call re-fetches. */
export function __resetMemoryBoundsCache(): void {
  cached = null;
  inflight = null;
}

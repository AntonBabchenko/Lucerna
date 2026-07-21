// Trailing-edge debounce for event-burst coalescing.
//
// Installing a mod with dependencies emits one Tauri event per jar; a
// handler that does a full refresh per event turns an 8-jar install into
// eight refresh storms within ~2 s. Collapse the burst: run once, `ms`
// after the last call. (Pattern origin: dep-graph.svelte.ts's reloadGraph.)

/**
 * Returns a debounced wrapper around `fn`: `call()` (re)arms a trailing
 * timer; `cancel()` disarms it (call from onDestroy so a pending timer
 * can't fire into a torn-down component).
 */
export function debounceTrailing(
  fn: () => void,
  ms: number,
): { call: () => void; cancel: () => void } {
  let timer: ReturnType<typeof setTimeout> | null = null;
  return {
    call() {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        fn();
      }, ms);
    },
    cancel() {
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
    },
  };
}

import { commands, type LoaderKind, type ModCompat, type ModLocalCompat } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

// Tooltip hint descriptor. The component maps these to i18n strings (it owns the
// instance loader/mc for interpolation).
export type CompatHint =
  | { key: 'fixAvailable' }
  | { key: 'loader'; detected: string }
  | { key: 'noRelease' };

// Owns proactive compatibility state. Layer 1 (offline scan) runs automatically
// on instance/mc/loader change; Layer 2 (live check) runs on the toolbar button.
// A mod is flagged iff the offline scan found a loader-family mismatch OR the
// live check returned Incompatible. Live Compatible never clears an offline
// mismatch (the installed jar is the wrong family); it only refines the hint.
export function createCompatCheck(
  getInstanceId: () => string | null,
  getMcVersion: () => string | null,
  getLoader: () => LoaderKind | null,
) {
  let offline = $state<Map<string, ModLocalCompat>>(new Map());
  let live = $state<Map<string, ModCompat>>(new Map());
  let checking = $state(false);
  let error = $state<string | null>(null);

  const incompatibleShas = $derived.by(() => {
    const out = new Set<string>();
    for (const [sha, lc] of offline) if (lc.loader_mismatch) out.add(sha);
    for (const [sha, lv] of live) if (lv.status.status === 'incompatible') out.add(sha);
    return out;
  });
  const incompatibleCount = $derived(incompatibleShas.size);

  function hintFor(sha1: string): CompatHint | null {
    const lc = offline.get(sha1);
    const lv = live.get(sha1);
    if (lc?.loader_mismatch) {
      if (lv?.status.status === 'compatible') return { key: 'fixAvailable' };
      return { key: 'loader', detected: lc.detected_loader ?? '?' };
    }
    if (lv?.status.status === 'incompatible') return { key: 'noRelease' };
    return null;
  }

  async function runOfflineScan() {
    const id = getInstanceId();
    const loader = getLoader();
    const mc = getMcVersion();
    if (!id || !loader || mc == null) {
      offline = new Map();
      return;
    }
    const r = await commands.scanInstanceModCompat(id, mc, loader);
    // Always replace (empty on error) — stale data from the previous instance
    // must not persist.
    offline = r.status === 'ok' ? new Map(r.data.map((x) => [x.sha1, x])) : new Map();
  }

  async function runLiveCheck() {
    const id = getInstanceId();
    const loader = getLoader();
    const mc = getMcVersion();
    if (!id || !loader || mc == null) return;
    checking = true;
    error = null;
    const r = await commands.checkInstanceModCompat(id, mc, loader);
    checking = false;
    if (r.status === 'error') {
      error = formatError(r.error);
      return;
    }
    live = new Map(r.data.map((x) => [x.sha1, x]));
  }

  // Re-scan offline + drop stale live results whenever the instance / mc /
  // loader changes. Wrapped in $effect.root for unit-testability (inert under
  // vitest); torn down via dispose() on unmount.
  let stopEffects: (() => void) | null = null;
  try {
    stopEffects = $effect.root(() => {
      $effect(() => {
        void getInstanceId();
        void getMcVersion();
        void getLoader();
        live = new Map();
        void runOfflineScan();
      });
    });
  } catch {
    /* no Svelte runtime (vitest) — effect inert; tests call runOfflineScan directly */
  }

  return {
    get incompatibleShas() {
      return incompatibleShas;
    },
    get incompatibleCount() {
      return incompatibleCount;
    },
    get checking() {
      return checking;
    },
    get error() {
      return error;
    },
    hintFor,
    runOfflineScan,
    runLiveCheck,
    dispose() {
      stopEffects?.();
    },
  };
}

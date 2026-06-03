import {
  commands,
  type InstalledMod,
  type ModUpdateCheck,
  type ModVersion,
} from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { t } from '$lib/i18n';
import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
import { get } from 'svelte/store';
import { updateCheckCache } from '../update-check-cache';

// Owns mod-update-check state. `updateChecks` is keyed by installed-mod sha1
// and seeded from the per-instance session cache so reopening the tab is
// instant; "Check for updates" forces a fresh check.
export function createUpdateCheck(getInstanceId: () => string | null, refresh: () => Promise<void>) {
  let updateChecks = $state<Map<string, ModUpdateCheck>>(new Map());
  let checking = $state(false);
  let showCfBanner = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const updateCount = $derived(
    [...updateChecks.values()].filter((c) => c.state.kind === 'update_available').length,
  );
  const updatableShas = $derived(
    new Set(
      [...updateChecks.values()]
        .filter((c) => c.state.kind === 'update_available')
        .map((c) => c.sha1),
    ),
  );

  // Empty the checks and drop the current instance's cache entry. Used after a
  // bulk update (versions moved → all checks are now stale) and by updateAll.
  function clearChecks() {
    const id = getInstanceId();
    updateChecks = new Map();
    if (id) updateCheckCache.delete(id);
  }

  // Seed the map from the session cache when the instance changes. Wrapped in
  // $effect.root for unit-testability; torn down via dispose() on unmount.
  let stopEffects: (() => void) | null = null;
  try {
    stopEffects = $effect.root(() => {
      $effect(() => {
        const id = getInstanceId();
        if (id) {
          const cached = updateCheckCache.get(id);
          updateChecks = cached ? new Map(cached.map((c) => [c.sha1, c])) : new Map();
        } else {
          updateChecks = new Map();
        }
      });
    });
  } catch {
    /* no Svelte runtime (vitest) — effect inert/unflushed, which is fine for unit tests */
  }

  async function checkUpdates() {
    const id = getInstanceId();
    if (!id) return;
    checking = true;
    error = null;
    const r = await commands.modsCheckUpdates(id);
    checking = false;
    if (r.status === 'error') {
      error = formatError(r.error);
      showCfBanner = false;
      return;
    }
    updateChecks = new Map(r.data.map((c) => [c.sha1, c]));
    updateCheckCache.set(id, r.data);
    const cfFailed = r.data.some((c) => c.source === 'curseforge' && c.state.kind === 'check_failed');
    if (cfFailed) {
      const s = await commands.modsGetCurseforgeKeyStatus();
      showCfBanner = s.status === 'ok' && s.data === 'missing';
    } else {
      showCfBanner = false;
    }
  }

  async function applyUpdate(sha1: string, target: ModVersion): Promise<boolean> {
    const id = getInstanceId();
    if (!id) return false;
    const r = await commands.modsUpdateOne(id, sha1, target);
    if (r.status === 'error') {
      error = formatError(r.error);
      return false;
    }
    return true;
  }

  async function updateOne(m: InstalledMod) {
    const id = getInstanceId();
    const c = updateChecks.get(m.sha1);
    if (!id || !c || c.state.kind !== 'update_available') return;
    busy = true;
    error = null;
    const ok = await applyUpdate(m.sha1, c.state.target);
    if (ok) {
      const next = new Map(updateChecks);
      next.delete(m.sha1);
      updateChecks = next;
      updateCheckCache.set(id, [...next.values()]);
    }
    busy = false;
    await refresh();
  }

  async function updateAll() {
    const id = getInstanceId();
    if (!id) return;
    const targets = [...updateChecks.values()].flatMap((c) =>
      c.state.kind === 'update_available' ? [{ sha1: c.sha1, target: c.state.target }] : [],
    );
    if (targets.length === 0) return;
    busy = true;
    error = null;
    let ok = 0;
    let failed = 0;
    for (const tgt of targets) {
      if (await applyUpdate(tgt.sha1, tgt.target)) ok++;
      else failed++;
    }
    // Every check is now stale (versions moved) — clear and let the user re-check.
    clearChecks();
    busy = false;
    await refresh();
    if (failed === 0) pushSuccess(get(t)('mods.installed.toastUpdated', { count: ok }));
    else pushWarning(get(t)('mods.installed.toastUpdatedFailed', { count: ok, failed }), []);
  }

  return {
    get updateChecks() {
      return updateChecks;
    },
    get checking() {
      return checking;
    },
    get updateCount() {
      return updateCount;
    },
    get updatableShas() {
      return updatableShas;
    },
    get showCfBanner() {
      return showCfBanner;
    },
    set showCfBanner(v: boolean) {
      showCfBanner = v;
    },
    get busy() {
      return busy;
    },
    get error() {
      return error;
    },
    set error(v: string | null) {
      error = v;
    },
    checkUpdates,
    updateOne,
    updateAll,
    clearChecks,
    dispose() {
      stopEffects?.();
    },
  };
}

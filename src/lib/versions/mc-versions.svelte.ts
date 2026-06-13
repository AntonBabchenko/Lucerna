// Self-healing owner of the Mojang version-manifest fetch. Extracted from
// `+page.svelte` so a transient startup failure (e.g. the network not yet up
// after the webview reloads on resume from sleep) no longer leaves a permanent
// error banner. The manifest is fetched once on mount; this composable adds two
// recovery paths so it self-heals instead of going stale:
//   1. the `window` `online` event (connectivity returned) → refetch;
//   2. a bounded backoff after a failed load (covers the common WebView case
//      where `navigator.onLine` is already true at remount, so no `online`
//      event fires, but the route comes up a couple seconds later).
// A monotonic `seq` guard (same idiom as createCompatCheck's `generation`)
// drops a slow stale response so it can't clobber a fresh result.
//
// No `$effect` here: state is plain `$state`; the listener and timers are
// imperative and torn down in `dispose()`. The consuming page instantiates one
// and calls `load()` on mount, `dispose()` on destroy.

import { commands, type VersionEntry } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { mcVersions } from '$lib/settings/state.svelte';

// Bounded auto-retry delays after a failed load (ms). After the last entry we
// stop auto-retrying; the `online` event and the manual Reload button remain.
const BACKOFF_MS = [2000, 5000, 10000];

export function createMcVersions() {
  let value = $state<VersionEntry[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(false);

  let seq = 0;
  let backoffStep = 0;
  let backoffTimer: ReturnType<typeof setTimeout> | null = null;
  let disposed = false;

  function cancelBackoff() {
    if (backoffTimer !== null) {
      clearTimeout(backoffTimer);
      backoffTimer = null;
    }
  }

  function scheduleBackoff() {
    if (backoffStep >= BACKOFF_MS.length) return; // exhausted — stop auto-retrying
    const delay = BACKOFF_MS[backoffStep];
    backoffStep += 1;
    cancelBackoff();
    backoffTimer = setTimeout(() => {
      backoffTimer = null;
      void attempt();
    }, delay);
  }

  // One fetch attempt. The seq guard drops the write if a newer load()/attempt()
  // started while this one awaited, or if the composable was disposed.
  async function attempt(): Promise<void> {
    const mine = ++seq;
    cancelBackoff();
    loading = true;
    const r = await commands.listVersions();
    if (mine !== seq || disposed) {
      // Superseded by a newer load(), or torn down mid-flight — drop the write,
      // but still clear `loading` so a disposed/superseded attempt can't leave a
      // stale spinner flag set.
      loading = false;
      return;
    }
    loading = false;
    if (r.status === 'ok') {
      value = r.data;
      // Publish to the shared rune so the McVersionCombobox in the mod / modpack
      // browsers and the Manage modal read the list without prop drilling.
      mcVersions.value = r.data;
      error = null;
      backoffStep = 0; // recovered — reset the schedule
    } else {
      error = formatError(r.error);
      scheduleBackoff();
    }
  }

  // Public fresh trigger (mount, Reload button, online event): resets the
  // backoff schedule, then runs an attempt.
  async function load(): Promise<void> {
    backoffStep = 0;
    await attempt();
  }

  // Refetch when connectivity returns — but only while in the error state, so a
  // healthy list is never disturbed.
  function onOnline() {
    if (error !== null) void load();
  }

  if (typeof window !== 'undefined') {
    window.addEventListener('online', onOnline);
  }

  return {
    get value() {
      return value;
    },
    get error() {
      return error;
    },
    get loading() {
      return loading;
    },
    load,
    // Hide the banner without cancelling self-heal (a later trigger can refill it).
    dismissError() {
      error = null;
    },
    dispose() {
      disposed = true;
      backoffStep = 0;
      cancelBackoff();
      if (typeof window !== 'undefined') {
        window.removeEventListener('online', onOnline);
      }
    },
  };
}

export type McVersions = ReturnType<typeof createMcVersions>;

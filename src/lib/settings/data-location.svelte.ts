// Shared owner of the data-root location status (`get_data_location`). Mirrors
// the `createMcVersions` composable shape (module-level runes wrapped in
// getters), but is a single shared instance rather than a per-consumer
// factory: `fellBack` gates UI in several unrelated places (the global
// fallback banner, ManageInstancesModal's create button, ServersView's create
// button, and Sidebar's Play/Install), so every consumer must read the SAME
// fetch rather than each firing its own `getDataLocation()` on mount.
//
// The Storage panel calls `refresh()` again after a successful
// `setDataLocation()` (though in practice that call restarts the app, so the
// refresh mostly matters for the failure path / the initial load).

import { commands, type DataLocationStatus } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

let status = $state<DataLocationStatus | null>(null);
let error = $state<string | null>(null);
let loading = $state(false);
let loaded = false;

async function refresh(): Promise<void> {
  loading = true;
  const r = await commands.getDataLocation();
  loading = false;
  if (r.status === 'ok') {
    status = r.data;
    error = null;
  } else {
    error = formatError(r.error);
  }
  loaded = true;
}

export const dataLocation = {
  get status() {
    return status;
  },
  get error() {
    return error;
  },
  get loading() {
    return loading;
  },
  /** True once `fell_back` is known to be true — the single gate every
   * create/Play/launch entry point reads. False (not blocked) until the
   * first successful load resolves, so the app never blocks on an unknown
   * state before startup has had a chance to report it. */
  get fellBack() {
    return status?.fell_back ?? false;
  },
  /** Load once (app startup). Subsequent calls are cheap no-ops guarded by
   * the caller; use `refresh()` to force a fresh load (e.g. after a
   * migration attempt fails and the user wants to retry). */
  async init(): Promise<void> {
    if (loaded) return;
    await refresh();
  },
  refresh,
};

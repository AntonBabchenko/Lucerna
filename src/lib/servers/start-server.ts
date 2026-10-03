// Starting a server from the UI, the one way every surface does it: the
// sidebar's Start, and the diagnosis banner's fixes that promise a start
// ("… & retry", "Accept EULA & start").

import { serverState } from './server-state.svelte';
import { serversUi } from './servers-ui.svelte';

/**
 * Show the server's Overview first, so the startup lines land on its console
 * card in view — and a refused start's message or a crash's banner with them —
 * then start it through the store's shared lifecycle (busy flag, error slot,
 * a diagnose after a failed start).
 */
export async function startShowingConsole(id: string): Promise<{ ok: boolean }> {
  serversUi.activeTab = 'overview';
  return serverState.start(id);
}

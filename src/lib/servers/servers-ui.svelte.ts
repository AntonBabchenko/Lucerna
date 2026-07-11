// Cross-component UI state for the Client/Servers launcher mode: which mode is
// active, which server the servers mode is looking at, the active management
// tab, and whether the creation wizard is open. A rune module (same pattern as
// compact.svelte.ts / settings/state.svelte.ts) because the state is written
// from the sidebar and read by the right panel and the page. Mode + selection
// persist to localStorage (webview-pref idiom, see fx/rainbow-fx.svelte.ts —
// no backend field, no bindings regen); tab + wizard flag are session-only.

export type UiMode = 'client' | 'servers';
export type ServerTab =
  | 'console'
  | 'connect'
  | 'general'
  | 'settings'
  | 'mods'
  | 'plugins'
  | 'hosting'
  | 'backups';

const MODE_KEY = 'lucerna.ui.mode';
const SELECTED_KEY = 'lucerna.ui.selectedServer';

export function loadMode(): UiMode {
  try {
    return localStorage.getItem(MODE_KEY) === 'servers' ? 'servers' : 'client';
  } catch {
    return 'client'; // localStorage unavailable — non-fatal
  }
}

export function loadSelectedServer(): string | null {
  try {
    return localStorage.getItem(SELECTED_KEY);
  } catch {
    return null;
  }
}

class ServersUi {
  mode = $state<UiMode>(loadMode());
  selectedServerId = $state<string | null>(loadSelectedServer());
  activeTab = $state<ServerTab>('console');
  creating = $state(false);

  setMode(mode: UiMode): void {
    this.mode = mode;
    try {
      localStorage.setItem(MODE_KEY, mode);
    } catch {
      /* localStorage unavailable — non-fatal */
    }
  }

  selectServer(id: string | null): void {
    this.selectedServerId = id;
    try {
      if (id === null) localStorage.removeItem(SELECTED_KEY);
      else localStorage.setItem(SELECTED_KEY, id);
    } catch {
      /* localStorage unavailable — non-fatal */
    }
  }

  /**
   * Keep the selection valid against the live server list: a stale id
   * (deleted server, fresh profile) falls back to the first server, or null
   * when there are none; a null selection auto-picks the first server so the
   * panel is never empty-with-servers. Writes only when the selection is
   * actually wrong, so it is safe to call from an $effect.
   */
  reconcile(ids: string[]): void {
    if (this.selectedServerId !== null && ids.includes(this.selectedServerId)) return;
    if (this.selectedServerId === null && ids.length === 0) return;
    this.selectServer(ids[0] ?? null);
  }
}

export const serversUi = new ServersUi();

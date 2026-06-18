// Per-instance "I collapsed the Needs-attention panel" flag, persisted so the
// choice survives restarts. Keyed by instance id.
//
// This is a pure presentation preference (whether to show a panel), not domain
// state, so it lives in the webview via localStorage — the same idiom the
// browser prefs and contextual tours use — rather than in instance.json.
//
// Semantics: collapsing is sticky. The panel stays hidden even when the set of
// warnings changes; the only way back is the restore affordance in the header
// (which calls setCollapsed(id, false)).

const KEY = 'lucerna.attentionCollapsed';

function load(): Set<string> {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return new Set();
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return new Set();
    return new Set(parsed.filter((v): v is string => typeof v === 'string'));
  } catch {
    return new Set();
  }
}

function persist(ids: Set<string>): void {
  try {
    localStorage.setItem(KEY, JSON.stringify([...ids]));
  } catch {
    /* localStorage unavailable — non-fatal */
  }
}

let collapsed = $state<Set<string>>(load());

export const attentionCollapse = {
  isCollapsed(id: string): boolean {
    return collapsed.has(id);
  },
  setCollapsed(id: string, value: boolean): void {
    if (value === collapsed.has(id)) return;
    const next = new Set(collapsed);
    if (value) next.add(id);
    else next.delete(id);
    collapsed = next;
    persist(next);
  },
  // Test-only: clear all state between cases.
  reset(): void {
    collapsed = new Set();
    try {
      localStorage.removeItem(KEY);
    } catch {
      /* noop */
    }
  },
};

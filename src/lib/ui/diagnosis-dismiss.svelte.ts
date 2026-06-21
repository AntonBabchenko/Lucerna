// Per-surface "I dismissed this diagnosis banner" memory, keyed by an opaque
// surface key (e.g. `server:<id>`, `log:<instanceId>`) → the dismissed
// diagnosis SIGNATURE.
//
// Semantics: a banner is hidden only while its *current* signature equals the
// dismissed one. So acknowledging one problem suppresses exactly that problem
// (across restarts), while a brand-new / different diagnosis re-surfaces on its
// own — the user is never silently blind to a fresh crash. The matching restore
// affordance clears the entry to bring the same known problem back.
//
// This is a pure presentation preference (whether to show a banner), not domain
// state, so it lives in the webview via localStorage — the same idiom as
// `attention-collapse.svelte.ts` and `browser-prefs.svelte.ts` — rather than in
// instance.json / the server record. See docs/DESIGN.md §10.

const KEY = 'lucerna.diagnosisDismissed';

function load(): Map<string, string> {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return new Map();
    const parsed: unknown = JSON.parse(raw);
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) return new Map();
    const out = new Map<string, string>();
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v === 'string') out.set(k, v);
    }
    return out;
  } catch {
    return new Map();
  }
}

function persist(map: Map<string, string>): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(Object.fromEntries(map)));
  } catch {
    /* localStorage unavailable — non-fatal */
  }
}

let dismissed = $state<Map<string, string>>(load());

export const diagnosisDismiss = {
  /**
   * True when this surface was dismissed for exactly this signature. A null
   * signature means the diagnosis has no stable identity yet → never suppress.
   */
  isDismissed(key: string, signature: string | null): boolean {
    if (signature === null) return false;
    return dismissed.get(key) === signature;
  },
  /** Remember that the user dismissed this exact diagnosis. No-op when unidentifiable. */
  dismiss(key: string, signature: string | null): void {
    if (signature === null) return;
    if (dismissed.get(key) === signature) return;
    const next = new Map(dismissed);
    next.set(key, signature);
    dismissed = next;
    persist(next);
  },
  /** Bring a dismissed banner back (restore affordance). */
  restore(key: string): void {
    if (!dismissed.has(key)) return;
    const next = new Map(dismissed);
    next.delete(key);
    dismissed = next;
    persist(next);
  },
  // Test-only: clear all state between cases.
  reset(): void {
    dismissed = new Map();
    try {
      localStorage.removeItem(KEY);
    } catch {
      /* noop */
    }
  },
};

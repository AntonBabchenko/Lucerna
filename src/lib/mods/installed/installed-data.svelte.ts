import {
  commands,
  type InstalledMod,
  type ModSource,
  type ModSummary,
  type PackOriginSummary,
} from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { mapLimit } from '../concurrency';

export type Row = {
  summary: ModSummary | null;
  installed: InstalledMod;
};

export type InstalledData = ReturnType<typeof createInstalledData>;

// Owns the installed-mod list for the active instance: loads the registry,
// the pack-origin chip data, runs the one-shot enrichment backfill, and
// resolves a ModSummary per platform mod (bounded concurrency). Every commit
// is guarded against an instance-switch race via `getInstanceId() === reqId`.
export function createInstalledData(getInstanceId: () => string | null) {
  let rows = $state<Row[]>([]);
  let packSummary = $state<PackOriginSummary | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function refresh(): Promise<void> {
    // Capture the instance this refresh is for. Several awaits happen below; if
    // the user switches instances mid-flight we must NOT commit this (now stale)
    // instance's data over the newer one. Every commit is guarded by
    // `getInstanceId() === reqId`.
    const reqId = getInstanceId();
    if (!reqId) {
      rows = [];
      return;
    }
    loading = true;
    error = null;

    // The list and the pack-origin summary are independent reads — run them in
    // parallel rather than sequentially.
    const [listRes, packRes] = await Promise.all([
      commands.modsListInstalled(reqId),
      commands.modsPackOriginSummary(reqId),
    ]);
    if (getInstanceId() !== reqId) return;
    if (listRes.status === 'error') {
      error = formatError(listRes.error);
      loading = false;
      return;
    }
    let list = listRes.data;
    const summary = packRes.status === 'ok' ? packRes.data : null;
    packSummary = summary;

    // Backfill: if this instance has modpack override-bundled mods that still
    // lack a platform identity and have never been hash-enriched, run one
    // enrichment pass and re-fetch the list. `enrich_attempted` flips true only
    // when EVERY platform the pass tried responded successfully, so this branch
    // can fire again on a later open until both platforms answered.
    if (
      summary &&
      list.some(
        (m) => m.source === null && !m.enrich_attempted && summary.mod_shas.includes(m.sha1),
      )
    ) {
      await commands.modsEnrichPackMods(reqId);
      const r2 = await commands.modsListInstalled(reqId);
      if (getInstanceId() !== reqId) return;
      if (r2.status === 'ok') list = r2.data;
    }

    // Fetch ModSummary for every platform-installed mod. Manual / unidentifiable
    // mods (source: null) skip the fetch and stay degraded. Bounded concurrency
    // keeps a big instance from firing dozens of parallel lookups at once.
    const enriched = await mapLimit(list, 6, async (m): Promise<Row> => {
      if (m.source === null || m.project_id === null) {
        return { summary: null, installed: m };
      }
      const p = await commands.modsProject(m.source as ModSource, m.project_id);
      if (p.status === 'ok') return { summary: p.data.summary, installed: m };
      // One retry — a transient hiccup usually clears (the backend caches a
      // concurrent success) so the mod renders normally rather than degraded.
      const retry = await commands.modsProject(m.source as ModSource, m.project_id);
      if (retry.status === 'ok') return { summary: retry.data.summary, installed: m };
      return { summary: null, installed: m };
    });
    if (getInstanceId() !== reqId) return;
    rows = enriched;
    loading = false;
  }

  // Blank the previous instance's data immediately on switch so stale content
  // never lingers while the new list loads, then trigger the load. Wrapped in
  // $effect.root so the factory can be unit-tested (no parent reactive context)
  // and the root is explicitly torn down via dispose() on component unmount —
  // never leaked, since ModBrowserTab unmounts on main-tab switch.
  let stopEffects: (() => void) | null = null;
  try {
    stopEffects = $effect.root(() => {
      $effect(() => {
        const id = getInstanceId();
        if (id) {
          rows = [];
          error = null;
          void refresh();
        }
      });
    });
  } catch {
    /* no Svelte runtime (vitest) — the effect is inert, which is what unit tests want */
  }

  return {
    get rows() {
      return rows;
    },
    get packSummary() {
      return packSummary;
    },
    get loading() {
      return loading;
    },
    get error() {
      return error;
    },
    // Writable: the view's single-row action handlers (toggle/uninstall/
    // switchVersion) set this to surface their own IPC errors through the
    // same banner. Kept on the composable so error display stays centralized.
    set error(v: string | null) {
      error = v;
    },
    refresh,
    dispose() {
      stopEffects?.();
    },
  };
}

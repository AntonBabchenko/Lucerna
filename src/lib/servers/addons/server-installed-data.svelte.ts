import { commands, type ModSource, type ModSummary } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { type EnrichedEntry, enrichedToCard, type ServerCardRow } from './server-card-adapter';

export type ServerAddonKind = 'mod' | 'plugin';
// `onDiskFilename` drives mutation commands; `card`/`reason` drive rendering.
export type ServerRow = {
  card: ServerCardRow;
  reason: string | null;
  onDiskFilename: string;
  sha1: string;
};

/** Pure: group identity-bearing rows' project_ids by source for batched lookup. */
export function groupProjectIdsBySource(
  rows: Array<{ source: ModSource | null; project_id: string | null }>,
): Map<ModSource, Set<string>> {
  const out = new Map<ModSource, Set<string>>();
  for (const r of rows) {
    if (r.source && r.project_id) {
      const set = out.get(r.source) ?? new Set<string>();
      set.add(r.project_id);
      out.set(r.source, set);
    }
  }
  return out;
}

/** Owns the enriched server Installed list for one server + kind: enriched list
 *  → one-shot hash-enrich backfill → re-list → batched ModSummary resolution →
 *  ServerRow[]. Every commit is guarded against a token race. */
export function createServerInstalledData(
  getServerId: () => string,
  kind: ServerAddonKind,
  getReloadToken: () => number,
) {
  let rows = $state<ServerRow[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let token = 0;

  async function listEnriched(id: string): Promise<EnrichedEntry[]> {
    const res =
      kind === 'mod'
        ? await commands.serverListModsEnriched(id)
        : await commands.serverListPluginsEnriched(id);
    if (res.status === 'error') throw new Error(formatError(res.error));
    return res.data as unknown as EnrichedEntry[];
  }

  async function refresh(): Promise<void> {
    const id = getServerId();
    const my = ++token;
    loading = true;
    error = null;
    try {
      let list = await listEnriched(id);
      if (list.some((e) => e.source === null)) {
        const en =
          kind === 'mod'
            ? await commands.serverEnrichMods(id)
            : await commands.serverEnrichPlugins(id);
        if (en.status === 'ok' && en.data > 0) list = await listEnriched(id);
      }
      if (token !== my) return;

      const byKey = new Map<string, ModSummary>();
      await Promise.all(
        [...groupProjectIdsBySource(list)].map(async ([source, ids]) => {
          const r = await commands.modsProjects(source, [...ids]);
          if (r.status === 'ok') for (const s of r.data) byKey.set(`${source}:${s.project_id}`, s);
        }),
      );
      if (token !== my) return;

      rows = list.map((e) => ({
        card: enrichedToCard(e, byKey),
        reason: 'reason' in e ? ((e as { reason: string | null }).reason ?? null) : null,
        onDiskFilename: e.on_disk_filename,
        sha1: e.sha1,
      }));
    } catch (e) {
      if (token === my) error = e instanceof Error ? e.message : String(e);
    } finally {
      if (token === my) loading = false;
    }
  }

  let stop: (() => void) | null = null;
  try {
    stop = $effect.root(() => {
      $effect(() => {
        void getServerId();
        void getReloadToken();
        void refresh();
      });
    });
  } catch {
    /* no Svelte runtime (vitest) — effect inert */
  }

  return {
    get rows() {
      return rows;
    },
    get loading() {
      return loading;
    },
    get error() {
      return error;
    },
    refresh,
    dispose() {
      stop?.();
    },
  };
}

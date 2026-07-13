import type { InstalledMod, ModSource, ModSummary } from '$lib/ipc/bindings';

// The {summary, installed} pair ModCard consumes — identical to the client's
// Row, reused so ModCard needs zero changes.
export type ServerCardRow = { summary: ModSummary | null; installed: InstalledMod };

// Structural shape of ServerModEntryEnriched / ServerPluginEntryEnriched.
// `on_disk_filename` is the CURRENT on-disk name (with `.disabled` when disabled)
// — used by mutation commands; `filename` is the base display name.
export type EnrichedEntry = {
  filename: string;
  on_disk_filename: string;
  disabled: boolean;
  sha1: string;
  source: ModSource | null;
  project_id: string | null;
  version_id: string | null;
  name: string | null;
  version_number: string | null;
};

/** Project an enriched server entry onto ModCard's {summary, installed} model.
 *  `summaryByKey` is keyed `${source}:${project_id}`. Rows without identity get
 *  summary=null → ModCard's degraded (filename) branch. */
export function enrichedToCard(
  e: EnrichedEntry,
  summaryByKey: Map<string, ModSummary>,
): ServerCardRow {
  const summary =
    e.source && e.project_id ? (summaryByKey.get(`${e.source}:${e.project_id}`) ?? null) : null;
  return {
    summary,
    installed: {
      filename: e.filename,
      sha1: e.sha1,
      source: e.source,
      project_id: e.project_id,
      version_id: e.version_id,
      name: e.name ?? e.filename,
      version_number: e.version_number,
      installed_at: '',
      enabled: !e.disabled,
    },
  };
}

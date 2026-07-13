import type {
  InstalledMod,
  ModSummary,
  ServerModEntryEnriched,
  ServerPluginEntryEnriched,
} from '$lib/ipc/bindings';

// The {summary, installed} pair ModCard consumes — identical to the client's
// Row, reused so ModCard needs zero changes.
export type ServerCardRow = { summary: ModSummary | null; installed: InstalledMod };

// The real binding union — mods carry a quarantine `reason`, plugins do not.
// `enrichedToCard` only reads fields common to both. `on_disk_filename` is the
// CURRENT on-disk name (with `.disabled` when disabled) — used by mutation
// commands; `filename` is the base display name.
export type EnrichedEntry = ServerModEntryEnriched | ServerPluginEntryEnriched;

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

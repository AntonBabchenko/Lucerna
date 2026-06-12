export type AttentionKind =
  | 'pick_version'
  | 'missing_mods'
  | 'incompatible'
  | 'integrity'
  | 'modpack_update';

export interface AttentionItem {
  kind: AttentionKind;
  count: number;
}

export interface AttentionInputs {
  mcVersionMissing: boolean;
  missingModsCount: number;
  incompatibleCount: number;
  /** Integrity problems to surface. Callers MUST pass 0 when integrity is
   *  healthy, absent, or stale (stale → shown as "not checked", not a problem). */
  integrityProblemCount: number;
  /** Whether the active instance's modpack has an update available. */
  hasModpackUpdate: boolean;
}

/** Build the ordered "needs attention" list from instance signals. */
export function buildAttentionItems(input: AttentionInputs): AttentionItem[] {
  const items: AttentionItem[] = [];
  if (input.mcVersionMissing) items.push({ kind: 'pick_version', count: 0 });
  if (input.missingModsCount > 0) {
    items.push({ kind: 'missing_mods', count: input.missingModsCount });
  }
  if (input.incompatibleCount > 0) {
    items.push({ kind: 'incompatible', count: input.incompatibleCount });
  }
  if (input.integrityProblemCount > 0) {
    items.push({ kind: 'integrity', count: input.integrityProblemCount });
  }
  if (input.hasModpackUpdate) items.push({ kind: 'modpack_update', count: 0 });
  return items;
}

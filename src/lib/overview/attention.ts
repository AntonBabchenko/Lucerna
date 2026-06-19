export type AttentionKind =
  | 'log_issue'
  | 'log_fix'
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
  /** Whether the latest log contains an unresolved problem worth surfacing. */
  hasLogIssue: boolean;
  /** Whether the surfaced log issue has a one-click repair available
   *  (diagnosis status 'actionable'). Only meaningful when hasLogIssue is true. */
  logFixAvailable: boolean;
}

/** Build the ordered "needs attention" list from instance signals. */
export function buildAttentionItems(input: AttentionInputs): AttentionItem[] {
  const items: AttentionItem[] = [];
  if (input.hasLogIssue) {
    items.push({ kind: input.logFixAvailable ? 'log_fix' : 'log_issue', count: 0 });
  }
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

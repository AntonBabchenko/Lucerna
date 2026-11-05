// The words for what a drop, a pick or a folder scan did (spec §5.3 item 5):
// how many landed, then each refusal with the files it stopped (`reasonLines`).
import { type ReportLine, reasonLines } from '$lib/format/reason-lines';
import type { Translate } from '$lib/i18n';
import type { ScanReport, SupplyRow } from '$lib/ipc/bindings';

export type SupplyResult = {
  message: string | null;
  details: ReportLine[];
  tone: 'success' | 'warning' | 'info';
};

function fileName(path: string): string {
  return (
    path
      .split(/[/]/)
      .filter((p) => p !== '')
      .pop() ?? path
  );
}

/** Why a row did not land, or null when it did (or waits for a Replace). */
export function refusal(t: Translate, r: SupplyRow): string | null {
  switch (r.kind) {
    case 'added':
    case 'already_there':
      return r.recorded ? null : t('modpacks.manual.result.notRecorded');
    case 'needs_replace':
      return null;
    case 'other_version':
      return t('modpacks.manual.result.otherVersion', { found: r.found, wanted: r.wanted });
    case 'not_in_pack':
      if (!r.asked) return t('modpacks.manual.result.notAsked');
      return r.found
        ? t('modpacks.manual.result.notInPackFound', { found: r.found })
        : t('modpacks.manual.result.notInPack');
    case 'cant_place':
      return t('modpacks.manual.result.cantPlace');
    case 'conflict':
      return t('modpacks.manual.result.conflict', { existing: r.existing });
    case 'unreadable':
      return t('modpacks.manual.result.unreadable', { reason: r.reason });
    case 'wrong_type':
      return t('modpacks.manual.result.wrongType');
    case 'changed':
      return t('modpacks.manual.result.changed');
    default: {
      const _exhaustive: never = r;
      return _exhaustive;
    }
  }
}

export function supplyResult(t: Translate, rows: readonly SupplyRow[]): SupplyResult {
  const landed = rows.filter((r) => r.kind === 'added' || r.kind === 'already_there').length;
  const waiting = rows.filter((r) => r.kind === 'needs_replace').length;
  const failures = rows.flatMap((r) => {
    const reason = refusal(t, r);
    return reason === null ? [] : [{ name: fileName(r.path), reason }];
  });
  const message =
    landed > 0
      ? t('modpacks.manual.result.added', { count: landed })
      : waiting > 0
        ? t('modpacks.manual.result.needsReplace', { count: waiting })
        : failures.length > 0
          ? t('modpacks.manual.result.noneAdded')
          : null;
  return {
    message,
    details: reasonLines(failures),
    tone: failures.length > 0 ? 'warning' : waiting > 0 ? 'info' : 'success',
  };
}

export function scanResult(t: Translate, report: ScanReport, stillPending: number): SupplyResult {
  const base = supplyResult(t, report.rows);
  const busy: ReportLine[] =
    report.skipped_unreadable > 0
      ? [t('modpacks.manual.scan.busy', { count: report.skipped_unreadable })]
      : [];
  const message = base.message ?? (stillPending > 0 ? t('modpacks.manual.scan.none') : null);
  return { message, details: [...base.details, ...busy], tone: base.tone };
}

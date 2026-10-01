import type { Translate } from '$lib/i18n';
import { displayLoader } from '$lib/instances/loader-display';
import type { DepViolation, LoaderKind } from '$lib/ipc/bindings';
import { violationAction, violationMessage } from '../violation-view';
import type { ModStatus, StatusFix, StatusReason } from './mod-status';

// The row's second line (spec §6.2): one short reason and at most one fix, in
// the colour of the row's level. Built from the row's ONE status (`statusOf`) —
// its first ranked reason and the fix the status chose for it — so the line can
// never name one reason and offer the repair of another.

export type RowFix =
  | { kind: 'enable'; label: string; violation: DepViolation }
  | { kind: 'install'; label: string; violation: DepViolation }
  | { kind: 'plan'; label: string; violation: DepViolation }
  | { kind: 'choose_version'; label: string };

export type RowProblem = {
  level: 'blocking' | 'warning';
  text: string;
  /** The longer compat sentence behind a warning (a tooltip); null for a pre-flight reason. */
  tooltip: string | null;
  /** Further reasons the «What stops the game» panel lists for this mod — «and N more». */
  more: number;
  fix: RowFix | null;
};

export type RowProblemInput = {
  t: Translate;
  /** The dependency's display name, as the Installed tab names it (never null: raw id last). */
  depName: (v: DepViolation) => string;
  loader: LoaderKind | null;
  mc: string | null;
  /** The mod has a platform identity, so «Choose version» can open its own version list. */
  canChooseVersion: boolean;
};

type CompatWarning = Extract<StatusReason, { source: 'compat' }>['hint'];

export function rowProblemOf(status: ModStatus, i: RowProblemInput): RowProblem | null {
  if (status.level !== 'blocking' && status.level !== 'warning') return null;
  const first = status.reasons[0];
  if (!first) return null;
  const fix = rowFix(status.fix, i);
  if (first.source === 'compat') {
    const hint = first.hint;
    return {
      level: status.level,
      text: warningText(i, hint),
      tooltip: hintSentence(i, hint),
      more: 0,
      fix,
    };
  }
  // «and N more» reveals the panel, so it counts what the panel lists: the pre-flight rows of a
  // BLOCKING mod. A compat warning behind them is not listed there; and a pre-flight reason at
  // the warning level is advisory (a self-completing pack still fetching) — the panel is quiet.
  const listed =
    status.level === 'blocking' ? status.reasons.filter((r) => r.source === 'preflight').length : 1;
  return {
    level: status.level,
    text: violationMessage(i.t, first.violation, i.depName(first.violation)),
    tooltip: null,
    more: listed - 1,
    fix,
  };
}

function rowFix(fix: StatusFix | null, i: RowProblemInput): RowFix | null {
  if (fix === null) return null;
  switch (fix.kind) {
    case 'enable-dependency':
      // The same rule as the panel's button: only a jar the report names can be switched on.
      return violationAction(fix.violation) === 'enable'
        ? { kind: 'enable', label: i.t('mods.preflight.enable'), violation: fix.violation }
        : null;
    case 'install-dependency':
      return {
        kind: 'install',
        label: i.t('mods.preflight.install', { dep: i.depName(fix.violation) }),
        violation: fix.violation,
      };
    case 'fix-version':
      return { kind: 'plan', label: i.t('mods.preflight.fixPlan'), violation: fix.violation };
    case 'choose-version':
    case 'migrate':
      // Another build of THIS mod. For a compat warning that is the per-mod face of the migration
      // plan; the plan itself is instance-wide and stays ONE button, the panel's «Fix incompatible
      // (N)» — a copy of it on every row would be N identical controls posing as per-mod actions.
      return i.canChooseVersion
        ? { kind: 'choose_version', label: i.t('mods.preflight.chooseVersion') }
        : null;
    case 'update':
      // Not a problem: the card's own Update action carries it.
      return null;
  }
}

// The WORD follows the evidence. A jar for another loader family is read off the file that will
// be launched: the loader skips it — certain for this mod, though the game starts. «No release»
// is only what the mod's page shows for this platform; the file itself makes no bounded
// statement, so it MAY not work.
function warningText(i: RowProblemInput, h: CompatWarning): string {
  switch (h.key) {
    case 'loader':
      return i.t('mods.installed.reasonLoaderMismatch', { loader: h.detected });
    case 'noRelease':
      return i.t('mods.installed.reasonNoRelease', { loader: loaderName(i), mc: i.mc ?? '' });
  }
}

function hintSentence(i: RowProblemInput, h: CompatWarning): string {
  switch (h.key) {
    case 'loader':
      return i.t('mods.installed.incompatHintLoader', {
        detected: h.detected,
        loader: loaderName(i),
      });
    case 'noRelease':
      return i.t('mods.installed.incompatHintNoRelease', { loader: loaderName(i), mc: i.mc ?? '' });
  }
}

const loaderName = (i: RowProblemInput): string => (i.loader ? displayLoader(i.loader) : '');

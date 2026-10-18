// What the Shared game settings rows, the launch dialog and the exit toast say
// (spec 2026-10-15 §B9), kept out of the components so each state is tested
// without rendering it. A failure is said as WHICH file and a reason class —
// never the OS or parser text, which is in the launcher log.

import type { Translate } from '$lib/i18n';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type {
  FileReason,
  Join,
  Kind,
  KindView,
  Repair,
  SyncFailure,
  SyncFile,
  WaitReason,
} from '$lib/ipc/bindings';

export const KIND_LABEL: Record<Kind, TranslationKey> = {
  servers: 'instance.settingsSync.kind.servers',
  history: 'instance.settingsSync.kind.history',
  hotbars: 'instance.settingsSync.kind.hotbars',
  options: 'instance.settingsSync.kind.options',
};

/** "the server list", in the case `{what}` takes in the sentences it fills. */
export const KIND_WHAT: Record<Kind, TranslationKey> = {
  servers: 'errors.settingsSyncWhat.servers',
  history: 'errors.settingsSyncWhat.history',
  hotbars: 'errors.settingsSyncWhat.hotbars',
  options: 'errors.settingsSyncWhat.options',
};

/** The launch dialog's "stop syncing this kind here" button, per kind. */
export const STOP_KIND: Record<Kind, TranslationKey> = {
  servers: 'launch.settingsSync.stopKind.servers',
  history: 'launch.settingsSync.stopKind.history',
  hotbars: 'launch.settingsSync.stopKind.hotbars',
  options: 'launch.settingsSync.stopKind.options',
};

/** `{what}` for a failure: a kind, or the sync settings themselves. */
export function failureWhat(f: Pick<SyncFailure, 'kind'>): TranslationKey {
  return f.kind === null ? 'errors.settingsSyncWhat.settings' : KIND_WHAT[f.kind];
}

const GAME_FILE: Record<Kind, TranslationKey> = {
  servers: 'errors.settingsSyncFile.game.servers',
  history: 'errors.settingsSyncFile.game.history',
  hotbars: 'errors.settingsSyncFile.game.hotbars',
  options: 'errors.settingsSyncFile.game.options',
};

const SHARED_FILE: Record<Kind, TranslationKey> = {
  servers: 'errors.settingsSyncFile.shared.servers',
  history: 'errors.settingsSyncFile.shared.history',
  hotbars: 'errors.settingsSyncFile.shared.hotbars',
  options: 'errors.settingsSyncFile.shared.options',
};

export const REASON: Record<FileReason, TranslationKey> = {
  damaged: 'errors.settingsSyncReason.damaged',
  no_access: 'errors.settingsSyncReason.noAccess',
  not_found: 'errors.settingsSyncReason.notFound',
  other: 'errors.settingsSyncReason.other',
};

/**
 * Which file, in words: this instance's own `servers.dat`, the shared server
 * list, this instance's sync settings or Lucerna's records. A game or shared
 * file with no kind cannot happen; it is named as the records, never guessed.
 */
export function fileKey(file: SyncFile, kind: Kind | null): TranslationKey {
  if (file === 'game' && kind !== null) return GAME_FILE[kind];
  if (file === 'shared' && kind !== null) return SHARED_FILE[kind];
  if (file === 'settings') return 'errors.settingsSyncFile.settings';
  return 'errors.settingsSyncFile.records';
}

/** "File: … Reason: …" — or the reason alone when no file is involved. */
export function failureDetail(
  tr: Translate,
  f: { file: SyncFile | null; kind: Kind | null; reason: FileReason },
): string {
  const reason = tr(REASON[f.reason]);
  return f.file === null
    ? tr('errors.settingsSyncDetailNoFile', { reason })
    : tr('errors.settingsSyncDetail', { file: tr(fileKey(f.file, f.kind)), reason });
}

const UNSUPPORTED: Record<
  Extract<KindView['availability'], { status: 'unsupported' }>['why'],
  TranslationKey
> = {
  history_too_old: 'instance.settingsSync.state.historyTooOld',
  hotbars_not_recorded: 'instance.settingsSync.state.hotbarsNotRecorded',
  hotbars_not_installed: 'instance.settingsSync.state.hotbarsNotInstalled',
  hotbars_not_release: 'instance.settingsSync.state.hotbarsNotRelease',
  options_too_old: 'instance.settingsSync.state.optionsTooOld',
  options_too_new: 'instance.settingsSync.state.optionsTooNew',
  options_not_release: 'instance.settingsSync.state.optionsNotRelease',
  options_not_installed: 'instance.settingsSync.state.optionsNotInstalled',
};

/** Why the options row waits: the game has not written options.txt yet. */
const WAITING: Record<WaitReason, TranslationKey> = {
  first_launch: 'instance.settingsSync.state.optionsFirstLaunch',
  game_save: 'instance.settingsSync.state.optionsGameSave',
};

/**
 * The line under a kind's checkbox, for the states the checkbox cannot say:
 * a join still to complete, a version that cannot sync it, hotbars older
 * than the shared ones, options waiting for the game to write its file.
 * `null` for plain on and plain off. What the version cannot do is said
 * whether the kind is on or off: one that is on waits for a version that can.
 */
export function kindStateKey(v: KindView): TranslationKey | null {
  if (v.availability.status === 'unreadable') return null;
  if (v.availability.status === 'unsupported') return UNSUPPORTED[v.availability.why];
  // Off, nothing is scheduled: the wait is said once the kind is on.
  if (v.availability.status === 'waiting') return v.enabled ? WAITING[v.availability.why] : null;
  if (v.availability.status === 'older_than_shared')
    return 'instance.settingsSync.state.olderThanShared';
  return v.enabled && v.join !== 'done' ? 'instance.settingsSync.state.joining' : null;
}

/**
 * The way out a failure's reason class has: a damaged shared copy is replaced
 * by this instance's file, a damaged own file by the shared copy, damaged
 * records start afresh. `null` for a passing failure (the next launch tries
 * again) and for the sync settings themselves (their own way out).
 */
export function repairFor(f: { file: SyncFile | null; reason: FileReason }): Repair | null {
  if (f.reason !== 'damaged') return null;
  if (f.file === 'shared') return 'replace_shared';
  if (f.file === 'game') return 'replace_own';
  if (f.file === 'records') return 'reset_records';
  return null;
}

export const REPAIR_LABEL: Record<Repair, TranslationKey> = {
  replace_shared: 'instance.settingsSync.repair.replaceShared',
  replace_own: 'instance.settingsSync.repair.replaceOwn',
  reset_records: 'instance.settingsSync.repair.resetRecords',
};

/** The launch dialog's button: the repair, then the launch. */
export const LAUNCH_REPAIR_LABEL: Record<Repair, TranslationKey> = {
  replace_shared: 'launch.settingsSync.repairAndLaunch.replaceShared',
  replace_own: 'launch.settingsSync.repairAndLaunch.replaceOwn',
  reset_records: 'launch.settingsSync.repairAndLaunch.resetRecords',
};

/**
 * What happens next, true for the reason class: a damaged file stays damaged
 * until it is replaced (and a damaged shared copy stops every instance that
 * syncs it), so only a passing failure is promised another try.
 */
export function nextStep(
  tr: Translate,
  f: { kind: Kind | null; file: SyncFile | null; reason: FileReason },
): string {
  switch (repairFor(f)) {
    case 'replace_shared':
      return tr('errors.settingsSyncNext.sharedDamaged');
    case 'replace_own':
      return f.kind === 'options'
        ? tr('errors.settingsSyncNext.gameDamagedOptions')
        : tr('errors.settingsSyncNext.gameDamaged');
    case 'reset_records':
      return tr('errors.settingsSyncNext.recordsDamaged');
    default:
      return tr('errors.settingsSyncNext.retry');
  }
}

/**
 * The whole line under a kind's checkbox, in words: [kindStateKey]'s states,
 * and a file this row needs that cannot be read (which file and why, and the
 * way out when there is one).
 */
export function rowLine(tr: Translate, v: KindView): string | null {
  const a = v.availability;
  if (a.status === 'unreadable') {
    const f = { file: a.file, kind: v.kind, reason: a.reason };
    const detail = failureDetail(tr, f);
    const line = tr('instance.settingsSync.state.unreadable', { detail });
    return repairFor(f) === null ? line : `${line} ${nextStep(tr, f)}`;
  }
  const key = kindStateKey(v);
  return key === null ? null : tr(key);
}

/**
 * The question when turning hotbars or options on, or `null` when turning a
 * kind on asks nothing (servers and history merge; with no shared copy, or no
 * own file, there is nothing to choose between). An instance older than the
 * shared hotbars cannot read them, so it is only offered to share its own (§B7).
 */
export function joinChoices(v: KindView): Exclude<Join, 'done'>[] | null {
  if ((v.kind !== 'hotbars' && v.kind !== 'options') || !v.join_asks) return null;
  return v.availability.status === 'older_than_shared'
    ? ['make_shared']
    : ['use_shared', 'make_shared'];
}

/** The kinds whose join asks, and the words of the question for each. */
export type AskingKind = Extract<Kind, 'hotbars' | 'options'>;

/**
 * The question, each choice's button, and one line per choice saying what it
 * does here AND to the other instances (the design review's D2): making this
 * instance's the shared ones replaces what every other instance takes next.
 */
export const JOIN_COPY: Record<
  AskingKind,
  {
    title: TranslationKey;
    body: TranslationKey;
    useShared: TranslationKey;
    useSharedResult: TranslationKey;
    makeShared: TranslationKey;
    makeSharedResult: TranslationKey;
  }
> = {
  hotbars: {
    title: 'instance.settingsSync.joinTitle',
    body: 'instance.settingsSync.joinBody',
    useShared: 'instance.settingsSync.useShared',
    useSharedResult: 'instance.settingsSync.useSharedResult',
    makeShared: 'instance.settingsSync.makeShared',
    makeSharedResult: 'instance.settingsSync.makeSharedResult',
  },
  options: {
    title: 'instance.settingsSync.joinTitleOptions',
    body: 'instance.settingsSync.joinBodyOptions',
    useShared: 'instance.settingsSync.useSharedOptions',
    useSharedResult: 'instance.settingsSync.useSharedResultOptions',
    makeShared: 'instance.settingsSync.makeSharedOptions',
    makeSharedResult: 'instance.settingsSync.makeSharedResultOptions',
  },
};

/** What a kind's sharing does and does not cover, said under it while it is off. */
export const RULE: Partial<Record<Kind, TranslationKey>> = {
  hotbars: 'instance.settingsSync.hotbarsRule',
  options: 'instance.settingsSync.optionsRule',
};

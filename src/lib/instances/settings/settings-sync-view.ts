// What the Shared game settings rows, the launch dialog and the exit toast say
// (spec 2026-10-15 §B9), kept out of the components so each state is tested
// without rendering it. A failure is said as WHICH file and a reason class —
// never the OS or parser text, which is in the launcher log.

import type { Translate } from '$lib/i18n';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { FileReason, Join, Kind, KindView, SyncFailure, SyncFile } from '$lib/ipc/bindings';

export const KIND_LABEL: Record<Kind, TranslationKey> = {
  servers: 'instance.settingsSync.kind.servers',
  history: 'instance.settingsSync.kind.history',
  hotbars: 'instance.settingsSync.kind.hotbars',
};

/** "the server list", in the case `{what}` takes in the sentences it fills. */
export const KIND_WHAT: Record<Kind, TranslationKey> = {
  servers: 'errors.settingsSyncWhat.servers',
  history: 'errors.settingsSyncWhat.history',
  hotbars: 'errors.settingsSyncWhat.hotbars',
};

/** The launch dialog's "stop syncing this kind here" button, per kind. */
export const STOP_KIND: Record<Kind, TranslationKey> = {
  servers: 'launch.settingsSync.stopKind.servers',
  history: 'launch.settingsSync.stopKind.history',
  hotbars: 'launch.settingsSync.stopKind.hotbars',
};

/** `{what}` for a failure: a kind, or the sync settings themselves. */
export function failureWhat(f: Pick<SyncFailure, 'kind'>): TranslationKey {
  return f.kind === null ? 'errors.settingsSyncWhat.settings' : KIND_WHAT[f.kind];
}

const GAME_FILE: Record<Kind, TranslationKey> = {
  servers: 'errors.settingsSyncFile.game.servers',
  history: 'errors.settingsSyncFile.game.history',
  hotbars: 'errors.settingsSyncFile.game.hotbars',
};

const SHARED_FILE: Record<Kind, TranslationKey> = {
  servers: 'errors.settingsSyncFile.shared.servers',
  history: 'errors.settingsSyncFile.shared.history',
  hotbars: 'errors.settingsSyncFile.shared.hotbars',
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
};

/**
 * The line under a kind's checkbox, for the states the checkbox cannot say:
 * a join still to complete, a version that cannot sync it, hotbars older
 * than the shared ones. `null` for plain on and plain off. What the version
 * cannot do is said whether the kind is on or off: one that is on waits for
 * a version that can.
 */
export function kindStateKey(v: KindView): TranslationKey | null {
  if (v.availability.status === 'unreadable') return null;
  if (v.availability.status === 'unsupported') return UNSUPPORTED[v.availability.why];
  if (v.availability.status === 'older_than_shared')
    return 'instance.settingsSync.state.olderThanShared';
  return v.enabled && v.join !== 'done' ? 'instance.settingsSync.state.joining' : null;
}

/**
 * The whole line under a kind's checkbox, in words: [kindStateKey]'s states,
 * and a file this row needs that cannot be read (which file and why).
 */
export function rowLine(tr: Translate, v: KindView): string | null {
  const a = v.availability;
  if (a.status === 'unreadable') {
    const detail = failureDetail(tr, { file: a.file, kind: v.kind, reason: a.reason });
    return tr('instance.settingsSync.state.unreadable', { detail });
  }
  const key = kindStateKey(v);
  return key === null ? null : tr(key);
}

/**
 * The hotbars question when turning a kind on, or `null` when turning it on
 * asks nothing (servers and history merge; hotbars with no shared copy, or no
 * own file, have nothing to choose between). An instance older than the shared
 * hotbars cannot read them, so it is only offered to share its own (§B7).
 */
export function joinChoices(v: KindView): Exclude<Join, 'done'>[] | null {
  if (v.kind !== 'hotbars' || !v.join_asks) return null;
  return v.availability.status === 'older_than_shared'
    ? ['make_shared']
    : ['use_shared', 'make_shared'];
}

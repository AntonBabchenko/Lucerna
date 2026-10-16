// What the Shared game settings rows say (spec 2026-10-15 §B9), kept out of
// the component so each state is tested without rendering it.

import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { Join, Kind, KindView, SyncFailure } from '$lib/ipc/bindings';

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

/** `{what}` for a failure: a kind, or the sync settings themselves. */
export function failureWhat(f: Pick<SyncFailure, 'kind'>): TranslationKey {
  return f.kind === null ? 'errors.settingsSyncWhat.settings' : KIND_WHAT[f.kind];
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
 * The line under a kind's checkbox; `null` when the checkbox says it all
 * (off, and nothing in the way). What the version cannot do is said whether
 * the kind is on or off: a kind that is on waits for a version that can.
 */
export function kindStateKey(v: KindView): TranslationKey | null {
  if (v.availability.status === 'unsupported') return UNSUPPORTED[v.availability.why];
  if (v.availability.status === 'older_than_shared')
    return 'instance.settingsSync.state.olderThanShared';
  if (!v.enabled) return null;
  return v.join === 'done'
    ? 'instance.settingsSync.state.on'
    : 'instance.settingsSync.state.joining';
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

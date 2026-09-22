// One contract for every instant-apply setting. The backend patch command
// persists one field-level change under one lock and returns the block AS
// PERSISTED; this store keeps that confirmed block, shows in-flight patches on
// top of it, sends them one at a time, and remembers — per field — what could
// not be saved, so the control that failed can say so.
//
// RED STUB (push 1): loads and sends, with no optimism, no chain, no failures.
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { type AppFile, commands, type GeneralSettings } from '$lib/ipc/bindings';
import { describeStoreError, formatError } from '$lib/ipc/format-error';

export type Field = keyof GeneralSettings;
export type Loaded =
  | { kind: 'pending' }
  | { kind: 'ok'; file: AppFile }
  | { kind: 'failed'; error: string };
export type FailureKind = 'refused' | 'unconfirmed' | 'not_loaded';
export type PatchOutcome = { ok: true } | { ok: false; kind: FailureKind; error: string };
type Pending = { seq: number; patch: Partial<GeneralSettings> };

export const appSettings = $state<{
  loaded: Loaded;
  pending: Pending[];
  failures: Partial<Record<Field, { kind: FailureKind; error: string }>>;
}>({ loaded: { kind: 'pending' }, pending: [], failures: {} });

/** The confirmed block with every in-flight patch applied in order; null
 *  until a load succeeded. */
export function generalDisplayed(): GeneralSettings | null {
  if (appSettings.loaded.kind !== 'ok') return null;
  return appSettings.loaded.file.general;
}

export function saveFailureKind(field: Field): FailureKind | null {
  return appSettings.failures[field]?.kind ?? null;
}

/** The generic sentence for a failed save of `field`; consents pick their own. */
export function saveFailure(field: Field): string | null {
  const f = appSettings.failures[field];
  if (!f) return null;
  return get(t)(
    f.kind === 'unconfirmed' ? 'settings.general.saveUnconfirmed' : 'settings.general.notSaved',
    { error: f.error },
  );
}

async function readNow(): Promise<void> {
  try {
    const r = await commands.appSettingsGet();
    appSettings.loaded =
      r.status === 'ok'
        ? { kind: 'ok', file: r.data }
        : { kind: 'failed', error: formatError(r.error) };
  } catch (e) {
    appSettings.loaded = { kind: 'failed', error: describeStoreError(e) };
  }
}

/** Startup and Retry only. */
export function loadAppSettings(): Promise<void> {
  appSettings.loaded = { kind: 'pending' };
  return readNow();
}

export async function patchGeneral(patch: Partial<GeneralSettings>): Promise<PatchOutcome> {
  try {
    const r = await commands.appSettingsPatchGeneral(patch);
    if (r.status === 'ok') {
      if (appSettings.loaded.kind === 'ok') {
        appSettings.loaded = {
          kind: 'ok',
          file: { ...appSettings.loaded.file, general: r.data },
        };
      }
      return { ok: true };
    }
    return { ok: false, kind: 'refused', error: formatError(r.error) };
  } catch (e) {
    return { ok: false, kind: 'unconfirmed', error: describeStoreError(e) };
  }
}

export function __resetAppSettingsForTest(): void {
  appSettings.loaded = { kind: 'pending' };
  appSettings.pending = [];
  appSettings.failures = {};
}

// One contract for every instant-apply setting. The backend patch command
// persists one field-level change under one lock and returns the block AS
// PERSISTED; this store keeps that confirmed block, shows in-flight patches on
// top of it, sends them one at a time, and remembers — per field — what could
// not be saved, so the control that failed can say so.
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

let seq = 0;
// Every load and patch runs behind the previous one: the optimistic order on
// screen IS the write order, and a patch issued during startup waits for the
// block it patches. Jobs never throw, so a failed one never wedges the chain.
let chain: Promise<unknown> = Promise.resolve();
function enqueue<T>(job: () => Promise<T>): Promise<T> {
  const p = chain.then(job, job);
  chain = p;
  return p;
}

/** The confirmed block with every in-flight patch applied in order; null
 *  until a load succeeded. Derived, never stored — a failed patch simply
 *  disappears from the list, so a newer patch to the same field keeps showing,
 *  arrays and objects included (no value comparison against a proxy). */
export function generalDisplayed(): GeneralSettings | null {
  if (appSettings.loaded.kind !== 'ok') return null;
  // The generated type marks the block optional (a serde default); a missing
  // block is "could not tell", never a default.
  const confirmed = appSettings.loaded.file.general;
  if (!confirmed) return null;
  return appSettings.pending.reduce((acc, p) => ({ ...acc, ...p.patch }), confirmed);
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

/** Startup and Retry only — never on a panel mount, which would flip the
 *  whole app to "unknown" under in-flight patches. */
export function loadAppSettings(): Promise<void> {
  appSettings.loaded = { kind: 'pending' };
  return enqueue(readNow);
}

/** Re-read the confirmed block without flipping to pending: after a call the
 *  transport lost, only the disk knows what landed. */
function refreshConfirmed(): Promise<void> {
  return enqueue(readNow);
}

// By sequence, never by identity: an entry pushed into $state is a proxy.
function drop(mine: Pending): void {
  appSettings.pending = appSettings.pending.filter((p) => p.seq !== mine.seq);
}
function fieldsOf(patch: Partial<GeneralSettings>): Field[] {
  return Object.keys(patch) as Field[];
}
function mark(patch: Partial<GeneralSettings>, kind: FailureKind, error: string): void {
  const next = { ...appSettings.failures };
  for (const f of fieldsOf(patch)) next[f] = { kind, error };
  appSettings.failures = next;
}
function clear(patch: Partial<GeneralSettings>): void {
  const next = { ...appSettings.failures };
  for (const f of fieldsOf(patch)) delete next[f];
  appSettings.failures = next;
}

export function patchGeneral(patch: Partial<GeneralSettings>): Promise<PatchOutcome> {
  const mine: Pending = { seq: ++seq, patch };
  appSettings.pending = [...appSettings.pending, mine]; // shown at once
  return enqueue(async (): Promise<PatchOutcome> => {
    if (appSettings.loaded.kind !== 'ok') {
      drop(mine);
      const error = appSettings.loaded.kind === 'failed' ? appSettings.loaded.error : '';
      mark(patch, 'not_loaded', error);
      return { ok: false, kind: 'not_loaded', error };
    }
    try {
      const r = await commands.appSettingsPatchGeneral(patch);
      if (r.status === 'ok') {
        if (appSettings.loaded.kind === 'ok') {
          appSettings.loaded = {
            kind: 'ok',
            file: { ...appSettings.loaded.file, general: r.data },
          };
        }
        drop(mine);
        clear(patch);
        return { ok: true };
      }
      // A typed Err: the file on disk is unchanged (tmp + rename has no
      // failure after the rename), so the confirmed block still holds.
      const error = formatError(r.error);
      drop(mine);
      mark(patch, 'refused', error);
      return { ok: false, kind: 'refused', error };
    } catch (e) {
      // The call itself failed: the write may or may not have landed. Say
      // that, and let the disk decide.
      const error = describeStoreError(e);
      drop(mine);
      mark(patch, 'unconfirmed', error);
      void refreshConfirmed();
      return { ok: false, kind: 'unconfirmed', error };
    }
  });
}

export function __resetAppSettingsForTest(): void {
  appSettings.loaded = { kind: 'pending' };
  appSettings.pending = [];
  appSettings.failures = {};
  chain = Promise.resolve();
  seq = 0;
}

// Shared owner of the data-root location status (`get_data_location`) AND of what this page knows
// about a data-folder move. Mirrors the `createMcVersions` composable shape (module-level runes
// wrapped in getters), but is a single shared instance rather than a per-consumer factory:
// `fellBack` gates UI in several unrelated places (the global fallback banner,
// ManageInstancesModal's create button, ServerSidebarSection / ServersPanel's create buttons, and
// Sidebar's Play/Install), so every consumer must read the SAME fetch rather than each firing its
// own `getDataLocation()` on mount.
//
// The move's state lives in the BACKEND (`DataLocationStatus.relocation`); this module only adds
// what one page knows beyond the last status read: the latest progress tick, whether this page
// started the move, and two recovery flags. That is what lets the app-level host
// (DataMoveHost.svelte) re-show the progress or final dialog after a reload (F5) or a tab switch.
//
// No self-effects on purpose: this is a directly-callable state machine; the reactive triggers
// (subscribe on mount, poll while running) live in the host component. `init()` / `refresh()`
// never subscribe — several suites mock the bindings without the progress event and reach
// `init()` through StoragePanel.

import { untrack } from 'svelte';
import {
  commands,
  type DataLocationStatus,
  type DataMigrationProgress,
  type DataMoveOutcome,
  events,
  type MovePhase,
  type RelocationStatus,
} from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

type RestartRequired = Extract<RelocationStatus, { kind: 'restart_required' }>;

/** What the app-level dialog renders. `details: null` means the move reported `restart_required`
 *  but the status read could not confirm it. */
export type RelocationView =
  | { kind: 'idle' }
  | { kind: 'running'; phase: MovePhase | null; progress: DataMigrationProgress | null }
  | { kind: 'restart_required'; details: RestartRequired | null };

const IDLE: RelocationStatus = { kind: 'idle' };
const IDLE_VIEW: RelocationView = { kind: 'idle' };

let status = $state<DataLocationStatus | null>(null);
let error = $state<string | null>(null);
let loading = $state(false);
let loaded = false;

/** Latest `dataMigrationProgress` payload; null until the first tick. */
let progress = $state<DataMigrationProgress | null>(null);
/** This page started the move and is still awaiting `setDataLocation`. */
let owned = $state(false);
/** The command said `restart_required`; the status read did not confirm it. */
let restartUnconfirmed = $state(false);
/** A move this page did not start ended without switching folders. */
let orphanEnded = $state(false);

function relocationOf(s: DataLocationStatus | null): RelocationStatus {
  // Always present from the real backend. Several unit suites still feed the pre-relocation
  // status shape through `getDataLocation`; there "no field" has to read as "no move" rather
  // than crash every gating component.
  return s?.relocation ?? IDLE;
}

async function refresh(): Promise<void> {
  loading = true;
  // Untracked on purpose. This runs synchronously inside whoever called us, and StoragePanel
  // reaches it from its mount `$effect` (through `init()`). A tracked read of `owned` / `status`
  // here would subscribe THAT effect to the store; the `status = r.data` below would then re-run
  // it, and every mount-time load in the panel (cache size, retention, data-root size, the
  // restart gate) would fire a second time.
  const watchedRun = untrack(() => !owned && relocationOf(status).kind === 'running');
  const r = await commands.getDataLocation();
  loading = false;
  if (r.status === 'ok') {
    status = r.data;
    error = null;
    const now = relocationOf(r.data).kind;
    if (now !== 'running') progress = null;
    if (now === 'restart_required') restartUnconfirmed = false;
    // running → idle on a page that does not own the command (it was reloaded mid-move): nobody
    // is awaiting the result, so say that the move ended.
    if (watchedRun && !owned && now === 'idle') orphanEnded = true;
  } else {
    error = formatError(r.error);
  }
  loaded = true;
}

/** Subscribe to the progress event. Returns the dispose function; the caller (DataMoveHost) owns
 *  the lifetime. Safe to dispose before `listen` has resolved. */
function attach(): () => void {
  let detached = false;
  let stop: (() => void) | null = null;
  events.dataMigrationProgress
    .listen((event) => {
      if (!detached) progress = event.payload;
    })
    .then((unlisten) => {
      // The owner went away while the subscription was being set up: release it now.
      if (detached) unlisten();
      else stop = unlisten;
    })
    .catch((e: unknown) => {
      // No live byte counter. The dialog still works: the host's poll keeps the phase current
      // from `get_data_location`; only the bar is missing.
      console.warn('[data-location] progress subscription failed:', e);
    });
  return () => {
    // Plain variables only — a $state read during teardown is stale.
    detached = true;
    stop?.();
    stop = null;
  };
}

export const dataLocation = {
  get status() {
    return status;
  },
  get error() {
    return error;
  },
  get loading() {
    return loading;
  },
  /** True once `fell_back` is known to be true — the single gate every create/Play/launch entry
   * point reads. False (not blocked) until the first successful load resolves, so the app never
   * blocks on an unknown state before startup has had a chance to report it. */
  get fellBack() {
    return status?.fell_back ?? false;
  },
  /** The OS-default data folder (where "Reset to default" moves the data); null until loaded. */
  get defaultDir(): string | null {
    return status?.default_dir ?? null;
  },
  /** The move as the app-level dialog should show it. `restart_required` wins over everything:
   *  it is the only state whose dialog is the way forward. */
  get relocation(): RelocationView {
    const rel = relocationOf(status);
    if (rel.kind === 'restart_required') return { kind: 'restart_required', details: rel };
    if (restartUnconfirmed) return { kind: 'restart_required', details: null };
    if (owned || rel.kind === 'running') {
      return {
        kind: 'running',
        phase: progress?.phase ?? (rel.kind === 'running' ? rel.phase : null),
        progress,
      };
    }
    return IDLE_VIEW;
  },
  get orphanEnded() {
    return orphanEnded;
  },
  ackOrphanEnded(): void {
    orphanEnded = false;
  },
  /** StoragePanel calls this right before `setDataLocation` for a real copy, so the dialog is up
   *  ("Preparing…") before the first progress tick. */
  moveStarted(): void {
    owned = true;
    progress = null;
    orphanEnded = false;
  },
  /** StoragePanel calls this when `setDataLocation` / `adoptDataLocation` returned (a clean move
   *  never returns). Order matters: the status is re-read while `owned` still holds the dialog
   *  up, so it never flickers through idle on its way to the final state. */
  async moveSettled(outcome: DataMoveOutcome | null): Promise<void> {
    await refresh();
    if (outcome?.kind === 'restart_required' && relocationOf(status).kind !== 'restart_required') {
      restartUnconfirmed = true;
    }
    owned = false;
  },
  /** Apply the `RelocationStatus` a retry returned, without another round trip. */
  applyRelocation(next: RelocationStatus): void {
    if (status) status = { ...status, relocation: next };
  },
  attach,
  /** Load once (app startup). Subsequent calls are cheap no-ops; use `refresh()` to force a fresh
   * load (the host polls it while a move runs; the Storage panel re-reads through `moveSettled`). */
  async init(): Promise<void> {
    if (loaded) return;
    await refresh();
  },
  refresh,
};

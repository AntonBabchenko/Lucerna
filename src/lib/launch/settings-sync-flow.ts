// The settings-sync half of a launch (spec 2026-10-15 §B3). A launch that
// stops on a failed sync, or on a game Lucerna lost track of, asks the user and
// then runs the SAME launch again with what they chose. The page owns the
// dialogs; this owns the loop, so the order of the questions and the options
// each answer sends are tested without the page.
import { repairFor } from '$lib/instances/settings/settings-sync-view';
import type { Error as IpcError, Kind, LaunchOptions, Repair } from '$lib/ipc/bindings';

export type LaunchResult = { status: 'ok'; data: number } | { status: 'error'; error: IpcError };

/** The sync failure a launch stopped on, as its dialog shows it. */
export type SyncFailedError = Extract<IpcError, { kind: 'settings_sync_failed' }>;

/**
 * "Launch without sync" / "Stop syncing <kind> here" / Cancel, or, for a
 * damaged file, the repair its reason class has, then the launch.
 */
export type SyncFailedChoice = 'skip' | 'stop' | 'repair' | 'cancel';

export interface SyncLaunchDeps {
  /** One launch attempt of the caller's own launch (its Quick Play target included). */
  launch: (options: LaunchOptions | null) => Promise<LaunchResult>;
  askSyncFailed: (error: SyncFailedError) => Promise<SyncFailedChoice>;
  /** "Is the game closed?" — true for "It's closed". */
  askGameClosed: () => Promise<boolean>;
  /** Stop syncing `kind` in this instance; `null` (an unreadable sync file) stops every kind. */
  stopSyncing: (
    kind: Kind | null,
  ) => Promise<{ status: 'ok'; data: null } | { status: 'error'; error: IpcError }>;
  /** The way out of a damaged sync file for `kind`. */
  repair: (
    kind: Kind,
    repair: Repair,
    gameClosedConfirmed: boolean,
  ) => Promise<{ status: 'ok'; data: unknown } | { status: 'error'; error: IpcError }>;
}

/** The last attempt's result, or `cancelled` when the user backed out of a question. */
export type SyncLaunchOutcome = LaunchResult | { status: 'cancelled' };

const CANCELLED = { status: 'cancelled' } as const;

/**
 * Launch, answering the two settings-sync stops in a loop. Every answer
 * changes the next attempt, so a question is never asked twice for the same
 * cause: "It's closed" confirms the game is gone, "Launch without sync" skips
 * every sync point, and a kind whose syncing was stopped and still fails is
 * reported, not asked about again.
 */
export async function launchWithSettingsSync(deps: SyncLaunchDeps): Promise<SyncLaunchOutcome> {
  let options: LaunchOptions | null = null;
  const stopped = new Set<Kind | null>();
  for (;;) {
    const result = await deps.launch(options);
    if (result.status === 'ok') return result;
    const e = result.error;
    if (e.kind === 'settings_sync_ask_game_closed' && !options?.game_closed_confirmed) {
      if (!(await deps.askGameClosed())) return CANCELLED;
      options = { ...(options ?? {}), game_closed_confirmed: true };
      continue;
    }
    if (e.kind === 'settings_sync_failed' && !stopped.has(e.sync_kind)) {
      const choice = await deps.askSyncFailed(e);
      if (choice === 'cancel') return CANCELLED;
      if (choice === 'skip') {
        options = { ...(options ?? {}), skip_settings_sync: true };
        continue;
      }
      const how = repairFor(e);
      if (choice === 'repair' && how !== null && e.sync_kind !== null) {
        const fixed = await deps.repair(e.sync_kind, how, options?.game_closed_confirmed ?? false);
        if (fixed.status === 'error') return fixed;
        // A kind that still fails after its repair is reported, not asked again.
        stopped.add(e.sync_kind);
        continue;
      }
      const stop = await deps.stopSyncing(e.sync_kind);
      if (stop.status === 'error') return stop;
      stopped.add(e.sync_kind);
      continue;
    }
    return result;
  }
}

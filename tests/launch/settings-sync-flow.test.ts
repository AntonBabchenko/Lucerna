import { describe, expect, it, vi } from 'vitest';
import type { Error as IpcError, LaunchOptions } from '$lib/ipc/bindings';
import {
  type LaunchResult,
  launchWithSettingsSync,
  type SyncLaunchDeps,
} from '$lib/launch/settings-sync-flow';

const OK: LaunchResult = { status: 'ok', data: 4242 };
const err = (error: IpcError): LaunchResult => ({ status: 'error', error });
const FAILED_SERVERS = err({
  kind: 'settings_sync_failed',
  sync_kind: 'servers',
  file: 'C:/i/.minecraft/servers.dat',
  reason: 'unreadable',
});
const ASK_CLOSED = err({ kind: 'settings_sync_ask_game_closed' });

/** A launch that answers each attempt from `results` in order and records its options. */
function deps(results: LaunchResult[], over: Partial<SyncLaunchDeps> = {}) {
  const seen: (LaunchOptions | null)[] = [];
  const d: SyncLaunchDeps = {
    launch: vi.fn(async (options) => {
      seen.push(options);
      const next = results.shift();
      if (!next) throw new Error('launched more often than expected');
      return next;
    }),
    askSyncFailed: vi.fn(async () => 'cancel' as const),
    askGameClosed: vi.fn(async () => false),
    stopSyncing: vi.fn(async () => ({ status: 'ok' as const, data: null })),
    ...over,
  };
  return { d, seen };
}

describe('launchWithSettingsSync', () => {
  it('launches once and asks nothing when the sync goes through', async () => {
    const { d, seen } = deps([OK]);
    expect(await launchWithSettingsSync(d)).toEqual(OK);
    expect(seen).toEqual([null]);
    expect(d.askSyncFailed).not.toHaveBeenCalled();
    expect(d.askGameClosed).not.toHaveBeenCalled();
  });

  it('"Launch without sync" runs the same launch again with every sync point skipped', async () => {
    const { d, seen } = deps([FAILED_SERVERS, OK], {
      askSyncFailed: vi.fn(async () => 'skip' as const),
    });
    expect(await launchWithSettingsSync(d)).toEqual(OK);
    expect(seen).toEqual([null, { skip_settings_sync: true }]);
    expect(d.stopSyncing).not.toHaveBeenCalled();
  });

  it('"Stop syncing" stops the failing kind, then launches with sync still on', async () => {
    const { d, seen } = deps([FAILED_SERVERS, OK], {
      askSyncFailed: vi.fn(async () => 'stop' as const),
    });
    expect(await launchWithSettingsSync(d)).toEqual(OK);
    expect(d.stopSyncing).toHaveBeenCalledWith('servers');
    expect(seen).toEqual([null, null]);
  });

  it('Cancel on the failure launches nothing more', async () => {
    const { d, seen } = deps([FAILED_SERVERS]);
    expect(await launchWithSettingsSync(d)).toEqual({ status: 'cancelled' });
    expect(seen).toHaveLength(1);
  });

  it('a failed stop is the outcome, and nothing launches after it', async () => {
    const busy: IpcError = { kind: 'instance_busy' };
    const { d, seen } = deps([FAILED_SERVERS], {
      askSyncFailed: vi.fn(async () => 'stop' as const),
      stopSyncing: vi.fn(async () => ({ status: 'error' as const, error: busy })),
    });
    expect(await launchWithSettingsSync(d)).toEqual({ status: 'error', error: busy });
    expect(seen).toHaveLength(1);
  });

  it('a kind that still fails after its syncing was stopped is reported, not asked again', async () => {
    const { d } = deps([FAILED_SERVERS, FAILED_SERVERS], {
      askSyncFailed: vi.fn(async () => 'stop' as const),
    });
    expect(await launchWithSettingsSync(d)).toEqual(FAILED_SERVERS);
    expect(d.askSyncFailed).toHaveBeenCalledTimes(1);
  });

  it('"It\'s closed" re-runs with the game confirmed closed; Cancel stops', async () => {
    const yes = deps([ASK_CLOSED, OK], { askGameClosed: vi.fn(async () => true) });
    expect(await launchWithSettingsSync(yes.d)).toEqual(OK);
    expect(yes.seen).toEqual([null, { game_closed_confirmed: true }]);

    const no = deps([ASK_CLOSED]);
    expect(await launchWithSettingsSync(no.d)).toEqual({ status: 'cancelled' });
    expect(no.seen).toHaveLength(1);
  });

  it('keeps both answers when the closed game is followed by a failed sync', async () => {
    const { d, seen } = deps([ASK_CLOSED, FAILED_SERVERS, OK], {
      askGameClosed: vi.fn(async () => true),
      askSyncFailed: vi.fn(async () => 'skip' as const),
    });
    expect(await launchWithSettingsSync(d)).toEqual(OK);
    expect(seen).toEqual([
      null,
      { game_closed_confirmed: true },
      { game_closed_confirmed: true, skip_settings_sync: true },
    ]);
  });

  it('any other launch error is returned as it is', async () => {
    const other = err({ kind: 'settings_sync_game_running' });
    const { d } = deps([other]);
    expect(await launchWithSettingsSync(d)).toEqual(other);
    expect(d.askSyncFailed).not.toHaveBeenCalled();
    expect(d.askGameClosed).not.toHaveBeenCalled();
  });
});

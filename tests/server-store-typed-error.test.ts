import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { Error as IpcError } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

// Same mock shape as tests/server-state.test.ts and
// tests/server-list-load-state.test.ts: replace the bindings module, stub the
// four events init() would listen on (we never call init(), but the module
// imports `events` at load).
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverList: vi.fn(),
    serverDiagnose: vi.fn(),
    serverBackupList: vi.fn(),
    serverImportInspect: vi.fn(),
    serverKill: vi.fn(),
  },
  events: {
    serverLogLine: { listen: vi.fn() },
    serverSpawned: { listen: vi.fn() },
    serverExited: { listen: vi.fn() },
    serverUploadProgress: { listen: vi.fn() },
  },
}));

import { commands } from '$lib/ipc/bindings';
import { serverState } from '$lib/servers/server-state.svelte';

describe('serverState wrappers hand back a typed IpcError', () => {
  beforeAll(() => locale.set('en'));

  it('diagnose: the failure branch narrows and feeds formatError with no cast', async () => {
    vi.mocked(commands.serverDiagnose).mockResolvedValue({
      status: 'error',
      error: { kind: 'server_not_running', id: 'a' },
    });
    const r = await serverState.diagnose('a');
    expect(r.ok).toBe(false);
    if (r.ok) throw new Error('unreachable: mocked an error Result');
    // Compile-time half of the assertion: `r.error` is `IpcError` here, so it
    // is accepted by formatError's parameter with NO cast and NO guard against
    // undefined. Under the old `{ ok: boolean; error?: unknown }` this line is
    // a `pnpm typecheck` error on both counts.
    expect(formatError(r.error)).toBe('This server is not running');
  });

  it('backupList: the success branch guarantees the payload', async () => {
    vi.mocked(commands.serverBackupList).mockResolvedValue({ status: 'ok', data: [] });
    const r = await serverState.backupList('a');
    expect(r.ok).toBe(true);
    if (!r.ok) throw new Error('unreachable: mocked an ok Result');
    // `r.list` is `BackupInfo[]`, not `BackupInfo[] | undefined` — the `?? []`
    // every caller wrote is now provably dead.
    expect(r.list).toEqual([]);
  });

  it('importInspect: the success branch guarantees the preview', async () => {
    const preview = {
      token: 'tok-1',
      detected_name: 'srv',
      mc_version: '1.21',
      loader: null,
      loader_version: null,
      can_launch_as_is: true,
      eula_in_source: true,
      mod_count: 0,
      world_present: false,
    };
    vi.mocked(commands.serverImportInspect).mockResolvedValue({
      status: 'ok',
      data: preview as never,
    });
    const r = await serverState.importInspect('C:/tmp/srv.zip');
    if (!r.ok) throw new Error('unreachable: mocked an ok Result');
    expect(r.preview.token).toBe('tok-1');
  });

  it('kill narrows server_not_running off the typed union, not a cast', async () => {
    vi.mocked(commands.serverList).mockResolvedValue({ status: 'ok', data: [] });
    vi.mocked(commands.serverKill).mockResolvedValue({
      status: 'error',
      error: { kind: 'server_not_running', id: 'a' },
    });
    // Behaviour pin (also covered by server-force-stop.test.ts; repeated here
    // because this is the call site whose cast the change deletes).
    const r = await serverState.kill('a');
    expect(r.ok).toBe(true);
    expect(serverState.actionErrorFor('a')).toBeUndefined();
  });

  it('an error kind outside the union is a compile error', () => {
    // @ts-expect-error — 'not_a_real_kind' is not a member of IpcError['kind'].
    const bogus: IpcError = { kind: 'not_a_real_kind' };
    expect(bogus).toBeDefined();
  });
});

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const ipc = vi.hoisted(() => ({
  modsVersions: vi.fn(),
  listFabricLoaders: vi.fn(),
  listNeoforgeLoaders: vi.fn(),
  listForgeLoaders: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: ipc }));

import {
  __resetShaderLoaderAvailabilityCache,
  cachedOutcomes,
  runChecks,
  SUCCESS_TTL_MS,
} from '$lib/mods/shader-loader-availability';
import { checkPlan } from '$lib/mods/shader-loader-hint';

const VERSION = { version_id: 'v' }; // only the list length matters here
const ok = <T>(data: T) => ({ status: 'ok' as const, data });
const err = (kind: string) => ({ status: 'error' as const, error: { kind } });

beforeEach(() => {
  ipc.modsVersions.mockResolvedValue(ok([]));
  ipc.listFabricLoaders.mockResolvedValue(ok([{ version: '0.16.0', stable: true, build: 1 }]));
  ipc.listNeoforgeLoaders.mockResolvedValue(ok([{ version: '21.1.1', stable: true, build: 1 }]));
  ipc.listForgeLoaders.mockResolvedValue(ok([{ version: '52.0.1', stable: true, build: 1 }]));
});

afterEach(() => {
  __resetShaderLoaderAvailabilityCache();
  vi.clearAllMocks();
  vi.useRealTimers();
});

describe('runChecks — shader checks', () => {
  it('asks Modrinth with the canonical project id, the MC version and the loader', async () => {
    await runChecks(checkPlan('neoforge'), '1.21.1');
    expect(ipc.modsVersions).toHaveBeenCalledWith('modrinth', 'YL57xq9U', '1.21.1', 'neoforge');
    expect(ipc.modsVersions).toHaveBeenCalledWith('modrinth', 'GchcoXML', '1.21.1', 'neoforge');
  });

  it('answers available / no_build / unknown', async () => {
    ipc.modsVersions.mockImplementation(async (_s: string, id: string) =>
      id === 'YL57xq9U' ? ok([VERSION]) : ok([]),
    );
    expect((await runChecks(checkPlan('neoforge'), '1.21.1')).shader).toEqual({
      'iris:neoforge': 'available',
      'oculus:neoforge': 'no_build',
    });
    __resetShaderLoaderAvailabilityCache();
    ipc.modsVersions.mockResolvedValue(err('network'));
    expect((await runChecks(checkPlan('fabric'), '1.21.1')).shader).toEqual({
      'iris:fabric': 'unknown',
    });
  });

  it('turns a thrown call into unknown without failing the other checks', async () => {
    ipc.modsVersions.mockImplementation(async (_s: string, id: string) => {
      if (id === 'GchcoXML') throw new Error('boom');
      return ok([VERSION]);
    });
    expect((await runChecks(checkPlan('neoforge'), '1.21.1')).shader).toEqual({
      'iris:neoforge': 'available',
      'oculus:neoforge': 'unknown',
    });
  });
});

describe('runChecks — mod loader install checks', () => {
  it('answers installable / not_installable (empty or loader_unavailable) / unknown', async () => {
    ipc.listFabricLoaders.mockResolvedValue(ok([{ version: '0.16.0', stable: true, build: 1 }]));
    ipc.listNeoforgeLoaders.mockResolvedValue(err('loader_unavailable'));
    ipc.listForgeLoaders.mockResolvedValue(err('forge_promotions_unavailable'));
    expect((await runChecks(checkPlan('vanilla'), '26.2')).install).toEqual({
      fabric: 'installable',
      neoforge: 'not_installable',
      forge: 'unknown',
    });
    __resetShaderLoaderAvailabilityCache();
    ipc.listFabricLoaders.mockResolvedValue(ok([]));
    ipc.listNeoforgeLoaders.mockRejectedValue(new Error('boom'));
    expect((await runChecks(checkPlan('vanilla'), '26.2')).install).toMatchObject({
      fabric: 'not_installable',
      neoforge: 'unknown',
    });
  });
});

describe('the success cache', () => {
  it('serves a remembered answer without asking again, but asks for another MC', async () => {
    ipc.modsVersions.mockResolvedValue(ok([VERSION]));
    await runChecks(checkPlan('fabric'), '1.21.1');
    expect(cachedOutcomes(checkPlan('fabric'), '1.21.1')).toEqual({
      shader: { 'iris:fabric': 'available' },
      install: {},
    });
    await runChecks(checkPlan('fabric'), '1.21.1');
    expect(ipc.modsVersions).toHaveBeenCalledTimes(1);
    await runChecks(checkPlan('fabric'), '1.21.4');
    expect(ipc.modsVersions).toHaveBeenCalledTimes(2);
  });

  it('never remembers unknown', async () => {
    ipc.modsVersions.mockResolvedValue(err('network'));
    await runChecks(checkPlan('fabric'), '1.21.1');
    expect(cachedOutcomes(checkPlan('fabric'), '1.21.1')).toBeNull();
    await runChecks(checkPlan('fabric'), '1.21.1');
    expect(ipc.modsVersions).toHaveBeenCalledTimes(2);
  });

  it('forgets an answer after the TTL', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-12T12:00:00Z'));
    ipc.modsVersions.mockResolvedValue(ok([]));
    await runChecks(checkPlan('fabric'), '1.21.1');
    vi.setSystemTime(new Date(Date.now() + SUCCESS_TTL_MS - 1));
    expect(cachedOutcomes(checkPlan('fabric'), '1.21.1')).not.toBeNull();
    vi.setSystemTime(new Date(Date.now() + 1));
    expect(cachedOutcomes(checkPlan('fabric'), '1.21.1')).toBeNull();
  });

  it('returns null from cachedOutcomes until every answer of the plan is remembered', async () => {
    ipc.modsVersions.mockImplementation(async (_s: string, id: string) =>
      id === 'YL57xq9U' ? ok([VERSION]) : err('network'),
    );
    await runChecks(checkPlan('neoforge'), '1.21.1');
    expect(cachedOutcomes(checkPlan('neoforge'), '1.21.1')).toBeNull();
  });
});

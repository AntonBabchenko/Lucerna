import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: { modsInstallMissingRequired: vi.fn() },
}));

import { commands } from '$lib/ipc/bindings';
import { installMissing } from '$lib/mods/preflight.svelte';

describe('installMissing', () => {
  beforeEach(() => vi.clearAllMocks());

  it('returns installed outcome on success', async () => {
    vi.mocked(commands.modsInstallMissingRequired).mockResolvedValue({
      status: 'ok',
      data: { kind: 'installed', name: 'Balm' },
    });
    const r = await installMissing('inst-1', 'balm');
    expect(r).toEqual({ kind: 'installed', name: 'Balm' });
    expect(commands.modsInstallMissingRequired).toHaveBeenCalledWith('inst-1', 'balm');
  });

  it('returns open_search outcome so the caller can open search', async () => {
    vi.mocked(commands.modsInstallMissingRequired).mockResolvedValue({
      status: 'ok',
      data: { kind: 'open_search', query: 'balm' },
    });
    const r = await installMissing('inst-1', 'balm');
    expect(r).toEqual({ kind: 'open_search', query: 'balm' });
  });

  it('maps an IPC error to an open_search fallback (never throws)', async () => {
    vi.mocked(commands.modsInstallMissingRequired).mockResolvedValue({
      status: 'error',
      error: { kind: 'mods_network', url: 'x', details: 'y' },
    });
    const r = await installMissing('inst-1', 'balm');
    expect(r).toEqual({ kind: 'open_search', query: 'balm' });
  });
});

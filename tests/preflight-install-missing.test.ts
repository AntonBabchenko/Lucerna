import { beforeEach, describe, expect, it, vi } from 'vitest';

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
    const r = await installMissing('inst-1', 'sha-waystones', 'balm');
    expect(r).toEqual({ kind: 'installed', name: 'Balm' });
    expect(commands.modsInstallMissingRequired).toHaveBeenCalledWith(
      'inst-1',
      'sha-waystones',
      'balm',
    );
  });

  it('returns open_search outcome so the caller can open search', async () => {
    vi.mocked(commands.modsInstallMissingRequired).mockResolvedValue({
      status: 'ok',
      data: { kind: 'open_search', query: 'balm' },
    });
    const r = await installMissing('inst-1', 'sha-waystones', 'balm');
    expect(r).toEqual({ kind: 'open_search', query: 'balm' });
  });

  // An error is not a miss. Turned into `open_search`, «already installed» read as "couldn't find
  // it — search", and a search invites the second copy that stops the game. Typed, the caller can
  // tell them apart (tests/installed-install-missing.test.ts).
  it('hands an IPC error back typed, never as a miss (never throws)', async () => {
    const error = { kind: 'mods_already_installed' as const, name: 'Balm' };
    vi.mocked(commands.modsInstallMissingRequired).mockResolvedValue({ status: 'error', error });
    const r = await installMissing('inst-1', 'sha-waystones', 'balm');
    expect(r).toEqual({ kind: 'failed', error });
  });

  // The sha1 of the mod that DECLARED the dependency is what lets the backend
  // read its platform metadata, which names the dependency's project outright.
  // Dropping it silently would put the resolver back on slug-guessing, which
  // for a slammed id like `forgeconfigapiport` finds nothing at all.
  it('passes the requiring mod through so the backend can read its declared deps', async () => {
    vi.mocked(commands.modsInstallMissingRequired).mockResolvedValue({
      status: 'ok',
      data: { kind: 'installed', name: 'Forge Config API Port' },
    });
    await installMissing('inst-1', 'sha-opac', 'forgeconfigapiport');
    expect(commands.modsInstallMissingRequired).toHaveBeenCalledWith(
      'inst-1',
      'sha-opac',
      'forgeconfigapiport',
    );
  });
});

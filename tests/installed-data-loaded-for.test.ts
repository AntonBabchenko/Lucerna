import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  modsListInstalled: vi.fn(),
  modsPackOriginSummary: vi.fn(),
  modsProjects: vi.fn(),
  modsEnrichPackMods: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));

import { createInstalledData } from '$lib/mods/installed/installed-data.svelte';

// The deps tour's one attempt per entry must not be spent on the empty list between a profile
// switch and the new rows' arrival (spec 2026-10-08 §3.1): `loadedFor` names the profile whose
// rows are loaded, once they are.
describe('createInstalledData loadedFor', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.modsPackOriginSummary.mockResolvedValue({ status: 'ok', data: null });
    mocks.modsProjects.mockResolvedValue({ status: 'ok', data: [] });
  });

  it('names the profile whose rows are loaded, once they are', async () => {
    mocks.modsListInstalled.mockResolvedValue({ status: 'ok', data: [] });
    const data = createInstalledData(() => 'i');
    expect(data.loadedFor).toBeNull();
    await data.refresh();
    expect(data.loadedFor).toBe('i');
    data.dispose();
  });

  it('stays null when the list could not be read', async () => {
    mocks.modsListInstalled.mockResolvedValue({ status: 'error', error: 'boom' });
    const data = createInstalledData(() => 'i');
    await data.refresh();
    expect(data.loadedFor).toBeNull();
    data.dispose();
  });
});

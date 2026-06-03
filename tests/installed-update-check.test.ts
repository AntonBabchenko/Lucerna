import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  modsCheckUpdates: vi.fn(),
  modsUpdateOne: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushSuccess: vi.fn(), pushWarning: vi.fn() }));
vi.mock('$lib/i18n', () => ({ t: { subscribe: () => () => {} } }));
vi.mock('svelte/store', () => ({ get: () => (k: string) => k }));
vi.mock('$lib/mods/update-check-cache', () => ({ updateCheckCache: new Map() }));

import { createUpdateCheck } from '$lib/mods/installed/update-check.svelte';

const check = (sha1: string, kind: string) => ({
  sha1, name: sha1.toUpperCase(), source: 'modrinth', project_id: 'p',
  current_version_id: 'v', current_version_number: '1.0',
  state: kind === 'update_available'
    ? { kind, target: { source: 'modrinth', project_id: 'p', version_id: 'v2', version_number: '2.0', name: 'n', loaders: ['fabric'] } }
    : { kind },
});

describe('createUpdateCheck', () => {
  beforeEach(async () => {
    const { updateCheckCache } = await import('$lib/mods/update-check-cache');
    (updateCheckCache as Map<string, unknown>).clear();
  });

  it('checkUpdates fills updateChecks and updateCount', async () => {
    mocks.modsCheckUpdates.mockResolvedValue({ status: 'ok', data: [check('a', 'update_available'), check('b', 'up_to_date')] });
    const u = createUpdateCheck(() => 'i', async () => {});
    await u.checkUpdates();
    expect(u.updateCount).toBe(1);
    expect([...u.updatableShas]).toEqual(['a']);
    expect(u.checking).toBe(false);
  });

  it('surfaces the CF key banner when a curseforge check fails and the key is missing', async () => {
    mocks.modsCheckUpdates.mockResolvedValue({ status: 'ok', data: [{ ...check('c', 'check_failed'), source: 'curseforge' }] });
    mocks.modsGetCurseforgeKeyStatus.mockResolvedValue({ status: 'ok', data: 'missing' });
    const u = createUpdateCheck(() => 'i', async () => {});
    await u.checkUpdates();
    expect(u.showCfBanner).toBe(true);
  });

  it('clearChecks empties updateChecks', async () => {
    mocks.modsCheckUpdates.mockResolvedValue({ status: 'ok', data: [check('a', 'update_available')] });
    const u = createUpdateCheck(() => 'i', async () => {});
    await u.checkUpdates();
    expect(u.updateCount).toBe(1);
    u.clearChecks();
    expect(u.updateCount).toBe(0);
    expect([...u.updatableShas]).toEqual([]);
  });
});

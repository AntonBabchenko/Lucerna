import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  modsCheckUpdates: vi.fn(),
  modsLastUpdateCheck: vi.fn(),
  modsListHolds: vi.fn(),
  modsSetHold: vi.fn(),
  modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
  updateMod: vi.fn(),
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));
vi.mock('$lib/tasks/adapters/mod-install', () => ({ updateMod: mocks.updateMod }));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: mocks.pushSuccess,
  pushWarning: mocks.pushWarning,
}));
vi.mock('$lib/i18n', () => ({ t: { subscribe: () => () => {} } }));
vi.mock('svelte/store', () => ({ get: () => (k: string) => k }));

import type { InstalledMod } from '$lib/ipc/bindings';
import { createUpdateCheck } from '$lib/mods/installed/update-check.svelte';
import {
  __resetUpdateCheckStoreForTests,
  storedUpdateCheck,
} from '$lib/mods/update-check-store.svelte';

const check = (sha1: string, kind: string, project = 'p') => ({
  sha1,
  name: sha1.toUpperCase(),
  source: 'modrinth',
  project_id: project,
  current_version_id: 'v',
  current_version_number: '1.0',
  state:
    kind === 'update_available'
      ? {
          kind,
          target: {
            source: 'modrinth',
            project_id: project,
            version_id: 'v2',
            version_number: '2.0',
            name: 'n',
            loaders: ['fabric'],
          },
        }
      : { kind },
});
const modA = {
  filename: 'a.jar',
  sha1: 'a',
  source: 'modrinth',
  project_id: 'p',
  version_id: 'v',
  name: 'A',
  version_number: '1.0',
  installed_at: '2026-01-01T00:00:00Z',
  enabled: true,
  enrich_attempted: false,
} as InstalledMod;
const make = () =>
  createUpdateCheck(
    () => 'i',
    async () => {},
  );
const summary = (deps: string[]) => ({
  status: 'ok',
  data: { primary_name: 'n', installed_dependencies: deps, details: [] },
});
const busy = { status: 'error', error: { kind: 'instance_busy' } };

beforeEach(() => {
  vi.clearAllMocks();
  __resetUpdateCheckStoreForTests();
  mocks.modsLastUpdateCheck.mockResolvedValue({ status: 'ok', data: null });
  mocks.modsListHolds.mockResolvedValue({ status: 'ok', data: [] });
});

describe('createUpdateCheck', () => {
  it('a fresh check fills the checks and stamps when it ran', async () => {
    mocks.modsCheckUpdates.mockResolvedValue({
      status: 'ok',
      data: [check('a', 'update_available'), check('b', 'up_to_date')],
    });
    const u = make();
    await u.checkUpdates();
    expect(u.updateCount).toBe(1);
    expect([...u.updatableShas]).toEqual(['a']);
    expect(u.checkedAtMs).not.toBeNull();
    expect(u.checking).toBe(false);
  });

  it('seeds the checks and their time from the stored check', async () => {
    mocks.modsLastUpdateCheck.mockResolvedValue({
      status: 'ok',
      data: { checked_at_secs: 1_000, results: [check('a', 'update_available')] },
    });
    const u = make();
    await u.loadStored('i');
    expect(u.updateCount).toBe(1);
    expect(u.checkedAtMs).toBe(1_000_000);
  });

  it('an unreadable stored check reads as "not checked"', async () => {
    mocks.modsLastUpdateCheck.mockRejectedValue(new Error('bridge down'));
    const u = make();
    await u.loadStored('i');
    expect(u.updateCount).toBe(0);
    expect(u.checkedAtMs).toBeNull();
  });

  it('a stored read that lands after a fresh check started never replaces it', async () => {
    let land: (v: unknown) => void = () => {};
    mocks.modsLastUpdateCheck.mockReturnValue(
      new Promise((r) => {
        land = r;
      }),
    );
    mocks.modsCheckUpdates.mockResolvedValue({
      status: 'ok',
      data: [check('a', 'update_available')],
    });
    const u = make();
    const stored = u.loadStored('i');
    await u.checkUpdates();
    land({ status: 'ok', data: { checked_at_secs: 1, results: [] } });
    await stored;
    expect(u.updateCount).toBe(1);
  });

  it('a check that ends after a profile switch lands on its own profile only', async () => {
    let land: (v: unknown) => void = () => {};
    mocks.modsCheckUpdates.mockReturnValue(
      new Promise((r) => {
        land = r;
      }),
    );
    let current = 'i';
    const u = createUpdateCheck(
      () => current,
      async () => {},
    );
    const running = u.checkUpdates();
    current = 'j';
    // The spinner belongs to the profile being checked.
    expect(u.checking).toBe(false);
    land({ status: 'ok', data: [check('a', 'update_available')] });
    await running;
    expect(u.updateCount).toBe(0);
    expect(storedUpdateCheck('i')?.results).toHaveLength(1);
  });

  it('holding a project drops its pending update and re-reads the holds', async () => {
    mocks.modsCheckUpdates.mockResolvedValue({
      status: 'ok',
      data: [check('a', 'update_available')],
    });
    mocks.modsSetHold.mockResolvedValue({ status: 'ok', data: null });
    const u = make();
    await u.checkUpdates();
    mocks.modsListHolds.mockResolvedValue({
      status: 'ok',
      data: [{ source: 'modrinth', project_id: 'p' }],
    });
    expect(await u.setHold(modA, true, 'A')).toBe(true);
    expect(mocks.modsSetHold).toHaveBeenCalledWith('i', 'modrinth', 'p', true);
    expect(u.updateCount).toBe(0);
    expect(u.isHeld(modA)).toBe(true);
  });

  it('releasing a hold reads the stored check again: its update is offered once more', async () => {
    mocks.modsSetHold.mockResolvedValue({ status: 'ok', data: null });
    const u = make();
    await u.loadStored('i');
    expect(u.updateCount).toBe(0);
    mocks.modsLastUpdateCheck.mockResolvedValue({
      status: 'ok',
      data: { checked_at_secs: 1_000, results: [check('a', 'update_available')] },
    });
    expect(await u.setHold(modA, false, 'A')).toBe(true);
    expect(mocks.modsSetHold).toHaveBeenCalledWith('i', 'modrinth', 'p', false);
    expect(u.updateCount).toBe(1);
  });

  it('a hold the profile refuses says it is busy, and changes nothing', async () => {
    mocks.modsCheckUpdates.mockResolvedValue({
      status: 'ok',
      data: [check('a', 'update_available')],
    });
    mocks.modsSetHold.mockResolvedValue(busy);
    const u = make();
    await u.checkUpdates();
    expect(await u.setHold(modA, true, 'A')).toBe(false);
    // A mod write under the shared claim: busy means another operation, never a running game (A9).
    expect(mocks.pushWarning).toHaveBeenCalledWith('mods.updates.holdFailed', ['mods.ops.busy']);
    expect(u.updateCount).toBe(1);
  });

  it('holds that cannot be read are unknown (null), not "none held"', async () => {
    mocks.modsListHolds.mockResolvedValue({ status: 'error', error: 'x' });
    const u = make();
    await u.loadHolds('i');
    expect(u.holds).toBeNull();
  });

  it('updateSelected updates only the chosen mods and names the dependencies they brought in', async () => {
    mocks.modsCheckUpdates.mockResolvedValue({
      status: 'ok',
      data: [check('a', 'update_available'), check('b', 'update_available', 'q')],
    });
    mocks.updateMod.mockResolvedValue(summary(['Lib X']));
    const u = make();
    await u.checkUpdates();
    await u.updateSelected(['a']);
    expect(mocks.updateMod).toHaveBeenCalledTimes(1);
    expect(mocks.updateMod.mock.calls[0][2]).toBe('a');
    expect([...u.updatableShas]).toEqual(['b']);
    expect(mocks.pushSuccess).toHaveBeenCalledWith('mods.installed.toastUpdated', [
      'mods.updates.installedDeps',
    ]);
  });

  it('a row update speaks only when it brought dependencies in, or failed — and then says why', async () => {
    mocks.modsCheckUpdates.mockResolvedValue({
      status: 'ok',
      data: [check('a', 'update_available'), check('b', 'update_available', 'q')],
    });
    const u = make();
    await u.checkUpdates();

    mocks.updateMod.mockResolvedValueOnce(summary([]));
    await u.updateOne(modA, 'Alpha');
    expect(mocks.pushSuccess).not.toHaveBeenCalled();
    expect(mocks.pushWarning).not.toHaveBeenCalled();
    expect([...u.updatableShas]).toEqual(['b']);

    mocks.updateMod.mockResolvedValueOnce(busy);
    await u.updateOne({ ...modA, sha1: 'b', project_id: 'q' }, 'Beta');
    expect(mocks.pushWarning).toHaveBeenCalledWith('mods.updates.updateFailed', ['mods.ops.busy']);
    // Still pending — nothing changed; the failure is a toast, not the tab's error box.
    expect([...u.updatableShas]).toEqual(['b']);
    expect(u.error).toBeNull();
  });

  it('surfaces the CF key banner when a curseforge check fails and the key is missing', async () => {
    mocks.modsCheckUpdates.mockResolvedValue({
      status: 'ok',
      data: [{ ...check('c', 'check_failed'), source: 'curseforge' }],
    });
    mocks.modsGetCurseforgeKeyStatus.mockResolvedValue({ status: 'ok', data: 'missing' });
    const u = make();
    await u.checkUpdates();
    expect(u.showCfBanner).toBe(true);
  });
});

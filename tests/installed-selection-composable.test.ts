import { describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  modsEnable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  modsDisable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  modsUninstall: vi.fn().mockResolvedValue({ status: 'ok', data: { token: 't', items: [] } }),
  modsUninstallMany: vi.fn().mockResolvedValue({ status: 'ok', data: { token: 't', items: [] } }),
  modsUpdateOne: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  modsFindOrphans: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  // Nothing depends on anything by default: the safe flip order names just the targets.
  modsRemovalImpact: vi.fn().mockImplementation(async (_i: string, sha1s: string[]) => ({
    status: 'ok',
    data: { dependents: [], order: sha1s },
  })),
  modsEnableImpact: vi.fn().mockImplementation(async (_i: string, sha1s: string[]) => ({
    status: 'ok',
    data: { requirements: [], order: sha1s },
  })),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => String(e) }));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
  pushActionToast: vi.fn(),
}));
vi.mock('$lib/i18n', () => ({ t: { subscribe: () => () => {} } }));
vi.mock('svelte/store', () => ({ get: () => (k: string) => k }));

import type { Row } from '$lib/mods/installed/installed-data.svelte';
import { createInstalledSelection } from '$lib/mods/installed/installed-selection.svelte';
import { answerDialog, opsDialog } from '$lib/mods/mod-ops.svelte';

const row = (sha1: string, enabled: boolean): Row => ({
  summary: null,
  installed: {
    filename: `${sha1}.jar`,
    sha1,
    source: 'modrinth',
    project_id: `p${sha1}`,
    version_id: 'v',
    name: sha1.toUpperCase(),
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled,
    enrich_attempted: false,
    requires: [],
  },
});

const noop = async () => {};
const over = (rows: Row[], refresh = noop, onMutated = () => {}) =>
  createInstalledSelection(
    () => rows,
    () => 'i',
    refresh,
    () => new Map(),
    onMutated,
  );

describe('createInstalledSelection', () => {
  it('toggles a row and computes allSelected', () => {
    const s = over([row('a', true), row('b', true)]);
    s.toggleSelect('a', true);
    expect(s.selected.has('a')).toBe(true);
    expect(s.allSelected).toBe(false);
    s.toggleSelectAll(true);
    expect(s.allSelected).toBe(true);
  });

  it('bulkSetEnabled flips only the rows that need it, through the guarded path', async () => {
    const s = over([row('a', false), row('b', true)]);
    s.toggleSelectAll(true);
    await s.bulkSetEnabled(true);
    // One impact check for the rows that change (plan A5): the already-enabled row is not asked about.
    expect(mocks.modsEnableImpact).toHaveBeenCalledWith('i', ['a']);
    expect(mocks.modsEnable).toHaveBeenCalledWith('i', 'a');
    expect(mocks.modsEnable).not.toHaveBeenCalledWith('i', 'b'); // already enabled
  });

  it('a bulk removal is one guarded call; onMutated and refresh follow once', async () => {
    const onMutated = vi.fn();
    const refresh = vi.fn(async () => {});
    mocks.modsUninstallMany.mockClear();
    const s = over([row('a', true), row('b', true)], refresh, onMutated);
    s.toggleSelectAll(true);
    await s.requestBulkUninstall();
    expect(mocks.modsRemovalImpact).toHaveBeenCalledWith('i', ['a', 'b']);
    expect(mocks.modsUninstallMany.mock.calls).toEqual([['i', ['a', 'b']]]);
    expect(onMutated).toHaveBeenCalledTimes(1);
    expect(refresh).toHaveBeenCalled();
    expect(s.selected.size).toBe(0);
  });

  it('a cancelled removal keeps the selection and changes nothing', async () => {
    mocks.modsRemovalImpact.mockResolvedValueOnce({
      status: 'ok',
      data: { dependents: [{ sha1: 'z', name: 'Z', needs: ['A'] }], order: ['z', 'a'] },
    });
    mocks.modsUninstall.mockClear();
    const s = over([row('a', true)]);
    // The composable's own effects make their first run — the instance-switch clear among them —
    // before anything is selected, as they do on mount in the app; unsettled, that run would
    // clear the selection mid-flow and hide what the cancel kept.
    await new Promise((r) => setTimeout(r, 0));
    s.toggleSelectAll(true);
    const done = s.requestBulkUninstall();
    await vi.waitFor(() => expect(opsDialog().view).not.toBeNull());
    answerDialog('cancel');
    await done;
    expect(mocks.modsUninstall).not.toHaveBeenCalled();
    expect(s.selected.has('a')).toBe(true);
  });

  it('bulkUpdate returns the updated sha1s and lists the dependencies they brought in', async () => {
    const toasts = await import('$lib/toasts/toasts.svelte');
    const target = { name: 'T', version_number: '2.0' } as never;
    const checks = new Map([
      ['a', { sha1: 'a', state: { kind: 'update_available', target } }],
      ['b', { sha1: 'b', state: { kind: 'update_available', target } }],
    ]) as never;
    mocks.modsUpdateOne
      .mockResolvedValueOnce({
        status: 'ok',
        data: { primary_name: 'A', installed_dependencies: ['Lib'], details: [] },
      })
      .mockResolvedValueOnce({ status: 'error', error: 'boom' });
    const rows = [row('a', true), row('b', true)];
    const s = createInstalledSelection(
      () => rows,
      () => 'i',
      noop,
      () => checks,
      () => {},
    );
    s.toggleSelectAll(true);
    expect(await s.bulkUpdate()).toEqual(['a']);
    // The mod on a line above its reason (`reasonLines`, plan §5d L4).
    expect(toasts.pushWarning).toHaveBeenCalledWith('mods.installed.toastUpdatedFailed', [
      'mods.updates.installedDeps',
      { names: 'B', reason: 'boom' },
    ]);
  });
});

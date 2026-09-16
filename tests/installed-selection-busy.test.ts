// Bulk enable/disable/uninstall in the Installed tab while the instance is held
// by a long operation. Every per-mod command takes the instance's shared
// maintenance claim, so during a pack update, a mod migration apply, a clone or
// a world migration each row is refused with InstanceBusy before a jar is
// touched. The toast used to carry only "Disabled 0 mods, 3 failed" — which
// cannot tell "an operation holds the instance, try again after it" apart from
// three unrelated failures — so the reasons ride along as its lines, once each.
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';

const mocks = vi.hoisted(() => ({
  modsEnable: vi.fn(),
  modsDisable: vi.fn(),
  modsUninstall: vi.fn(),
  modsFindOrphans: vi.fn(),
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsEnable: mocks.modsEnable,
    modsDisable: mocks.modsDisable,
    modsUninstall: mocks.modsUninstall,
    modsFindOrphans: mocks.modsFindOrphans,
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: mocks.pushSuccess,
  pushWarning: mocks.pushWarning,
}));

import type { Row } from '$lib/mods/installed/installed-data.svelte';
import { createInstalledSelection } from '$lib/mods/installed/installed-selection.svelte';

const BUSY = 'An operation is already in progress, or the game is running.';

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

const busy = { status: 'error' as const, error: { kind: 'instance_busy' as const } };
const ok = { status: 'ok' as const, data: null };

function selectionOver(rows: Row[]) {
  const s = createInstalledSelection(
    () => rows,
    () => 'inst1',
    async () => {},
    () => new Map(),
    () => {},
  );
  s.toggleSelectAll(true);
  return s;
}

describe('bulk mod actions refused while the instance is held', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => vi.clearAllMocks());

  it('a bulk disable names the busy reason once, not just a failure count', async () => {
    mocks.modsDisable.mockResolvedValue(busy);
    const s = selectionOver([row('a', true), row('b', true), row('c', true)]);

    await s.bulkSetEnabled(false);

    expect(mocks.modsDisable).toHaveBeenCalledTimes(3);
    expect(mocks.pushSuccess).not.toHaveBeenCalled();
    expect(mocks.pushWarning).toHaveBeenCalledTimes(1);
    const [title, lines] = mocks.pushWarning.mock.calls[0] as [string, string[]];
    expect(title).toContain('3 failed');
    expect(lines).toEqual([BUSY]);
  });

  it('a bulk uninstall names the busy reason alongside a different failure', async () => {
    mocks.modsFindOrphans.mockResolvedValue({ status: 'ok', data: [] });
    mocks.modsUninstall
      .mockResolvedValueOnce(busy)
      .mockResolvedValueOnce({
        status: 'error',
        error: { kind: 'mods_not_found', source: 'installed' },
      })
      .mockResolvedValueOnce(busy);
    const s = selectionOver([row('a', true), row('b', true), row('c', true)]);

    await s.requestBulkUninstall();
    await s.confirmBulkUninstall([]);

    expect(mocks.pushWarning).toHaveBeenCalledTimes(1);
    const [, lines] = mocks.pushWarning.mock.calls[0] as [string, string[]];
    expect(lines).toHaveLength(2);
    expect(lines[0]).toBe(BUSY);
    expect(lines[1]).not.toBe(BUSY);
  });

  it('a fully successful bulk enable still reports success with no warning', async () => {
    mocks.modsEnable.mockResolvedValue(ok);
    const s = selectionOver([row('a', false), row('b', false)]);

    await s.bulkSetEnabled(true);

    expect(mocks.pushWarning).not.toHaveBeenCalled();
    expect(mocks.pushSuccess).toHaveBeenCalledTimes(1);
  });
});

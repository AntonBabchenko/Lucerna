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
  modsUninstallMany: vi.fn(),
  modsFindOrphans: vi.fn(),
  modsRemovalImpact: vi.fn(),
  modsEnableImpact: vi.fn(),
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
  pushActionToast: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsEnable: mocks.modsEnable,
    modsDisable: mocks.modsDisable,
    modsUninstall: mocks.modsUninstall,
    modsUninstallMany: mocks.modsUninstallMany,
    modsFindOrphans: mocks.modsFindOrphans,
    modsRemovalImpact: mocks.modsRemovalImpact,
    modsEnableImpact: mocks.modsEnableImpact,
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: mocks.pushSuccess,
  pushWarning: mocks.pushWarning,
  pushActionToast: mocks.pushActionToast,
}));

import type { Row } from '$lib/mods/installed/installed-data.svelte';
import { createInstalledSelection } from '$lib/mods/installed/installed-selection.svelte';

// A mod write takes the SHARED claim, so the refusal means a long operation holds
// the profile — never that the game runs (plan A9): mod-ops' own copy, not the
// shared `instance_busy` text.
const BUSY =
  'Another operation — such as a modpack update, a migration or a clone — is using this profile. Try again once it finishes.';

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
  beforeEach(() => {
    vi.clearAllMocks();
    // Nothing depends on anything: the safe flip order names just the targets.
    mocks.modsRemovalImpact.mockImplementation(async (_i: string, sha1s: string[]) => ({
      status: 'ok',
      data: { dependents: [], order: sha1s },
    }));
    mocks.modsEnableImpact.mockImplementation(async (_i: string, sha1s: string[]) => ({
      status: 'ok',
      data: { requirements: [], order: sha1s },
    }));
  });

  it('a bulk disable names the busy reason once, not just a failure count', async () => {
    mocks.modsDisable.mockResolvedValue(busy);
    const s = selectionOver([row('a', true), row('b', true), row('c', true)]);

    await s.bulkSetEnabled(false);

    // The flip follows the backend's safe order, so the first refusal ends it: the
    // two mods after it are never tried, and count as failed for the same reason.
    expect(mocks.modsDisable).toHaveBeenCalledTimes(1);
    expect(mocks.pushSuccess).not.toHaveBeenCalled();
    expect(mocks.pushWarning).toHaveBeenCalledTimes(1);
    const [title, lines] = mocks.pushWarning.mock.calls[0] as [string, string[]];
    expect(title).toContain('3 failed');
    expect(lines).toEqual([BUSY]);
  });

  // A bulk removal is now ONE command (one token, all-or-nothing on the backend), so a refusal
  // is one reason, reported once — and there is nothing to undo.
  it('a refused bulk removal names the busy reason and offers no Undo', async () => {
    mocks.modsFindOrphans.mockResolvedValue({ status: 'ok', data: [] });
    mocks.modsUninstallMany.mockResolvedValue(busy);
    const s = selectionOver([row('a', true), row('b', true), row('c', true)]);

    await s.requestBulkUninstall();

    expect(mocks.modsUninstallMany).toHaveBeenCalledTimes(1);
    expect(mocks.pushWarning).toHaveBeenCalledTimes(1);
    const [title, lines] = mocks.pushWarning.mock.calls[0] as [string, string[]];
    expect(title).toBe("Couldn't remove 3 mods");
    expect(lines).toEqual([BUSY]);
    expect(mocks.pushActionToast).not.toHaveBeenCalled();
  });

  it('a fully successful bulk enable still reports success with no warning', async () => {
    mocks.modsEnable.mockResolvedValue(ok);
    const s = selectionOver([row('a', false), row('b', false)]);

    await s.bulkSetEnabled(true);

    expect(mocks.pushWarning).not.toHaveBeenCalled();
    expect(mocks.pushSuccess).toHaveBeenCalledTimes(1);
  });
});

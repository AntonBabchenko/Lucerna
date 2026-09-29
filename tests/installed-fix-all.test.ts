/**
 * «Fix all (N)» on the Installed tab's «What stops the game» panel runs the same repair as the
 * Play gate (fix-all.ts), then a FRESH pre-flight — only it says which rows are gone — and says
 * «Fixed N of M» in one toast — with why the steps that failed failed.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { formatError } from '$lib/ipc/format-error';

const alpha = vi.hoisted(() => ({
  filename: 'alpha.jar',
  sha1: 'a',
  source: 'modrinth',
  project_id: 'PA',
  version_id: 'v',
  name: 'Alpha',
  version_number: '1.0',
  installed_at: '2026-01-01T00:00:00Z',
  enabled: true,
  enrich_attempted: false,
  requires: [],
}));

const h = vi.hoisted(() => ({
  instanceDependencyPreflight: vi.fn(),
  modsEnableImpact: vi.fn(),
  modsEnable: vi.fn(),
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [
        alpha,
        {
          ...alpha,
          filename: 'balm.jar',
          sha1: 'balm-sha',
          project_id: 'PBALM',
          name: 'Balm',
          enabled: false,
        },
      ],
    }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    instanceDependencyPreflight: h.instanceDependencyPreflight,
    modsResolveDepNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // The repair switches the disabled provider on through mod-ops, which asks the impact first —
    // WITH its safe flip `order`.
    modsEnableImpact: h.modsEnableImpact,
    modsEnable: h.modsEnable,
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: h.pushSuccess,
  pushWarning: h.pushWarning,
  pushActionToast: vi.fn(),
  pushInfo: vi.fn(),
}));

import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

const balmDisabled = {
  dependent_sha1: 'a',
  dependent_name: 'Alpha',
  dep_id: 'balm',
  kind: 'required_disabled',
  installed_version: null,
  needed: '',
  needed_desc: { raw: '', family: 'maven', alternatives: [], unparseable: false, soft: false },
  provider_project: null,
  provider_sha1: 'balm-sha',
  family: null,
};

// A DISTINCT instance id per case: `preflightCache` is a per-instance LRU.
const props = (instanceId: string) => ({
  instanceId,
  mcVersion: '1.21.1',
  loader: 'neoforge' as const,
});

beforeEach(() => {
  h.instanceDependencyPreflight.mockReset();
  h.pushSuccess.mockReset();
  h.pushWarning.mockReset();
  h.modsEnableImpact.mockResolvedValue({
    status: 'ok',
    data: { requirements: [], order: ['balm-sha'] },
  });
  h.modsEnable.mockResolvedValue({ status: 'ok', data: null });
});

describe('«Fix all» on the Installed panel', () => {
  it('repairs, re-checks, and says what the re-check no longer reports', async () => {
    h.instanceDependencyPreflight
      .mockResolvedValueOnce({ status: 'ok', data: { violations: [balmDisabled] } })
      .mockResolvedValue({ status: 'ok', data: { violations: [] } });
    render(InstalledModsView, { props: props('fix-all-clean') });
    await fireEvent.click(await screen.findByRole('button', { name: 'Fix all (1)' }));
    await waitFor(() => expect(h.pushSuccess).toHaveBeenCalledWith('Fixed 1 of 1', []));
    expect(h.modsEnableImpact).toHaveBeenCalledWith('fix-all-clean', ['balm-sha']);
    expect(h.modsEnable).toHaveBeenCalledWith('fix-all-clean', 'balm-sha');
    // A fresh pre-flight after the repair, never the report it started from.
    expect(h.instanceDependencyPreflight).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(screen.queryByTestId('preflight-panel')).toBeNull());
  });

  it('a row the re-check still reports is not counted as fixed — and the toast says why', async () => {
    h.instanceDependencyPreflight.mockResolvedValue({
      status: 'ok',
      data: { violations: [balmDisabled] },
    });
    const denied = { kind: 'io' as const, path: 'mods', details: 'denied' };
    h.modsEnable.mockResolvedValue({ status: 'error', error: denied });
    render(InstalledModsView, { props: props('fix-all-partial') });
    await fireEvent.click(await screen.findByRole('button', { name: 'Fix all (1)' }));
    await waitFor(() =>
      expect(h.pushWarning).toHaveBeenCalledWith('Fixed 0 of 1', [formatError(denied)]),
    );
    expect(h.pushSuccess).not.toHaveBeenCalled();
  });
});

/**
 * Installing, uninstalling or toggling a mod must re-run the compatibility scan.
 *
 * It stopped doing so in #332: the shared store keys on (instance, mc, loader),
 * and none of those events changes that key, so an unforced `runOfflineScan()`
 * is deduplicated away and neither surface ever notices a newly-added
 * wrong-loader jar. The PR that gave the two surfaces one store simultaneously
 * disabled the trigger that kept it fresh.
 *
 * This pins the `{ force: true }` at the call site, not just the pass-through in
 * the composable — deleting either literal in InstalledModsView restores the
 * regression, and nothing else in the suite would notice.
 */
import { render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const listeners = vi.hoisted(() => ({
  modInstalled: null as null | (() => void),
  modToggle: null as null | (() => void),
}));

const mod = vi.hoisted(() => (sha1: string, projectId: string, name: string) => ({
  filename: `${sha1}.jar`,
  sha1,
  source: 'modrinth',
  project_id: projectId,
  version_id: 'v',
  name,
  version_number: '1.0',
  installed_at: '2026-01-01T00:00:00Z',
  enabled: true,
  enrich_attempted: false,
  requires: [],
}));

const mocks = vi.hoisted(() => ({ scanInstanceModCompat: vi.fn() }));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [mod('a', 'PA', 'Alpha')] }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    scanInstanceModCompat: mocks.scanInstanceModCompat,
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    modInstalled: {
      listen: (cb: () => void) => {
        listeners.modInstalled = cb;
        return Promise.resolve(() => {});
      },
    },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: {
      listen: (cb: () => void) => {
        listeners.modToggle = cb;
        return Promise.resolve(() => {});
      },
    },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import { invalidateCompatScan } from '$lib/mods/compat-scan.svelte';
import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

const props = { instanceId: 'i', mcVersion: '1.21.1', loader: 'neoforge' as const };

describe('a mod-set change forces a compatibility rescan', () => {
  beforeEach(() => {
    mocks.scanInstanceModCompat.mockReset();
    mocks.scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [] });
    invalidateCompatScan();
  });

  it('rescans after modInstalled, even though the scan key is unchanged', async () => {
    render(InstalledModsView, { props });
    await waitFor(() => expect(mocks.scanInstanceModCompat).toHaveBeenCalledTimes(1));

    listeners.modInstalled?.();

    // The handler is trailing-debounced by 150ms.
    await waitFor(() => expect(mocks.scanInstanceModCompat).toHaveBeenCalledTimes(2), {
      timeout: 3000,
    });
    // Same key both times — which is exactly why the second call needs `force`.
    expect(mocks.scanInstanceModCompat).toHaveBeenNthCalledWith(2, 'i', '1.21.1', 'neoforge');
  });

  it('rescans after modToggle', async () => {
    render(InstalledModsView, { props });
    await waitFor(() => expect(mocks.scanInstanceModCompat).toHaveBeenCalledTimes(1));

    listeners.modToggle?.();

    await waitFor(() => expect(mocks.scanInstanceModCompat).toHaveBeenCalledTimes(2), {
      timeout: 3000,
    });
  });
});

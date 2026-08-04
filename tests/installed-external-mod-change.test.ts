/**
 * A jar that arrives in `mods/` from outside the launcher must refresh what is
 * DERIVED from the mod list — the pre-flight report, the compatibility scan, the
 * dependency graph — and must NOT re-request the list itself.
 *
 * The second half is the whole loop guard: `mods_list_installed` is what emits
 * `ModsReconciled`, so a handler that calls it again would feed itself. This test
 * fails the moment someone adds `data.refresh()` to that handler.
 */
import { render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const listeners = vi.hoisted(() => ({ modsReconciled: null as null | (() => void) }));

const mod = vi.hoisted(() => ({
  filename: 'a.jar',
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

const mocks = vi.hoisted(() => ({
  modsListInstalled: vi.fn(),
  instanceDependencyPreflight: vi.fn(),
  scanInstanceModCompat: vi.fn(),
  modsDependencyGraph: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: mocks.modsListInstalled,
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: mocks.modsDependencyGraph,
    instanceDependencyPreflight: mocks.instanceDependencyPreflight,
    scanInstanceModCompat: mocks.scanInstanceModCompat,
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: {
      listen: (cb: () => void) => {
        listeners.modsReconciled = cb;
        return Promise.resolve(() => {});
      },
    },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import { invalidateCompatScan } from '$lib/mods/compat-scan.svelte';
import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

const props = { instanceId: 'i', mcVersion: '1.21.1', loader: 'neoforge' as const };

describe('an external change to mods/ refreshes only the derived views', () => {
  beforeEach(() => {
    for (const m of Object.values(mocks)) m.mockReset();
    mocks.modsListInstalled.mockResolvedValue({ status: 'ok', data: [mod] });
    mocks.instanceDependencyPreflight.mockResolvedValue({
      status: 'ok',
      data: { violations: [], pack_completion: null },
    });
    mocks.scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [] });
    mocks.modsDependencyGraph.mockResolvedValue({ status: 'ok', data: { roots: [] } });
    invalidateCompatScan();
  });

  it('re-runs the pre-flight and the compatibility scan', async () => {
    render(InstalledModsView, { props });
    await waitFor(() => expect(mocks.instanceDependencyPreflight).toHaveBeenCalled());
    const preflightBefore = mocks.instanceDependencyPreflight.mock.calls.length;
    const compatBefore = mocks.scanInstanceModCompat.mock.calls.length;

    listeners.modsReconciled?.();

    await waitFor(
      () => {
        expect(mocks.instanceDependencyPreflight.mock.calls.length).toBeGreaterThan(
          preflightBefore,
        );
        expect(mocks.scanInstanceModCompat.mock.calls.length).toBeGreaterThan(compatBefore);
      },
      { timeout: 3000 },
    );
  });

  it('does NOT re-request the mod list — that is what emitted the event', async () => {
    render(InstalledModsView, { props });
    await waitFor(() => expect(mocks.modsListInstalled).toHaveBeenCalled());
    const listBefore = mocks.modsListInstalled.mock.calls.length;

    listeners.modsReconciled?.();

    // Give the 150ms trailing debounce room to fire, then assert the list was
    // left alone. A `data.refresh()` in that handler makes this fail.
    await new Promise((r) => setTimeout(r, 600));
    expect(mocks.modsListInstalled.mock.calls.length).toBe(listBefore);
  });
});

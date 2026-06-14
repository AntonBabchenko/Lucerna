import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// Capture each listener callback at module-eval time via vi.hoisted so the
// (also-hoisted) vi.mock factory can write through to this registry — same
// scaffold as installed-mods-view.test.ts.
const listeners = vi.hoisted(() => ({
  modInstalled: null as null | (() => void),
  modUninstalled: null as null | (() => void),
  modToggle: null as null | (() => void),
}));

// 60 platform-installed mods → more than one page at the default
// installedPageSize of 50 (60 / 50 = 2 pages). Each has a resolvable
// project_id so a ModSummary lookup succeeds and the row renders.
const mods = vi.hoisted(() =>
  Array.from({ length: 60 }, (_, i) => ({
    filename: `m${i}.jar`,
    sha1: `s${i}`,
    source: 'modrinth',
    project_id: `p${i}`,
    version_id: 'v',
    name: `Mod ${String(i).padStart(2, '0')}`,
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
    requires: [],
  })),
);

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: mods }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsProject: vi.fn().mockImplementation((_s: string, id: string) =>
      Promise.resolve({
        status: 'ok',
        data: {
          summary: {
            source: 'modrinth',
            project_id: id,
            slug: id,
            name: `Proj ${id}`,
            summary: '',
            icon_url: null,
            downloads: 1,
            author: 'x',
            updated_at: null,
          },
          description: '',
          website_url: null,
        },
      }),
    ),
    modsDisable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsUninstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {
    modInstalled: {
      listen: (cb: () => void) => {
        listeners.modInstalled = cb;
        return Promise.resolve(() => {});
      },
    },
    modUninstalled: {
      listen: (cb: () => void) => {
        listeners.modUninstalled = cb;
        return Promise.resolve(() => {});
      },
    },
    modToggle: {
      listen: (cb: () => void) => {
        listeners.modToggle = cb;
        return Promise.resolve(() => {});
      },
    },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import { browserPrefs } from '$lib/mods/browser-prefs.svelte';
import InstalledModsView from '$lib/mods/InstalledModsView.svelte';

describe('Installed pagination footer (unified)', () => {
  beforeEach(() => {
    browserPrefs.pageSize = 20; // catalog (Browse) size
    browserPrefs.installedPageSize = 50; // installed size
  });

  it('shows the shared "Page X of Y" label and a PageSizePicker', async () => {
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    // 60 mods / 50 per page = 2 pages. The 60 modsProject lookups resolve
    // over several microtask ticks, so give waitFor headroom.
    await waitFor(() => expect(screen.getByText(/Page 1 of 2/i)).toBeTruthy(), { timeout: 3000 });
    expect(screen.getByTestId('page-size-50')).toBeTruthy();
    expect(screen.getByTestId('page-size-100')).toBeTruthy();
  });

  it('changing the installed page size persists independently of the catalog size', async () => {
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await waitFor(() => expect(screen.getByTestId('page-size-100')).toBeTruthy(), {
      timeout: 3000,
    });
    await fireEvent.click(screen.getByTestId('page-size-100'));
    expect(browserPrefs.installedPageSize).toBe(100);
    expect(browserPrefs.pageSize).toBe(20); // catalog size untouched
  });
});

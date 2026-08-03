/**
 * The Overview's «N несовместимых модов» indicator deep-links into the Installed
 * tab's incompatible view. `requestedFilter` shipped in #332 with no test, and
 * the filter was in fact reverted before a single row existed:
 *
 *   MainTabs renders AddonsTab under {#if active === 'mod_browser'}, so arriving
 *   from the Overview always MOUNTS it fresh → `data.rows` is `[]` for the whole
 *   mount flush → `counts.incompatible` is 0 (the predicate has nothing to run
 *   over) → writing `viewFilter` re-runs the auto-reset effect → it reads that 0
 *   as "none" and reverts to 'all'.
 *
 * So the user landed on the full unfiltered list — the exact outcome #332's
 * commit message says it fixed. These two cases pin both halves: the filter must
 * survive a not-yet-loaded list, and must still fall back when the set really is
 * empty (which is what the auto-reset exists for).
 */
import { render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

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
    modsListInstalled: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [mod('a', 'PA', 'Alpha'), mod('b', 'PB', 'Bravo')],
    }),
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
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import { invalidateCompatScan } from '$lib/mods/compat-scan.svelte';
import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

// A manual suspect: loader-mismatched and not live-checkable, which is the
// offline-decidable verdict both surfaces count.
const mismatch = (sha1: string) => ({
  sha1,
  loader_mismatch: true,
  live_checkable: false,
  detected_loader: 'Fabric',
});

const props = {
  instanceId: 'i',
  mcVersion: '1.21.1',
  loader: 'neoforge' as const,
  requestedFilter: 'incompatible' as const,
};

describe('Overview deep-link into the incompatible view', () => {
  beforeEach(() => {
    mocks.scanInstanceModCompat.mockReset();
    // The scan is an app-wide singleton shared with the Overview.
    invalidateCompatScan();
  });

  it('keeps the requested filter when the row list has not loaded yet', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [mismatch('a')] });

    render(InstalledModsView, { props });

    // Alpha is the incompatible one; Bravo must stay filtered out. Before the
    // fix the filter was already back to 'all' by this point and BOTH rows
    // rendered.
    await waitFor(() => {
      expect(document.querySelector('[data-mod-row="modrinth:PA"]')).not.toBeNull();
    });
    expect(document.querySelector('[data-mod-row="modrinth:PB"]')).toBeNull();
  });

  it('still falls back to "all" when nothing is actually incompatible', async () => {
    mocks.scanInstanceModCompat.mockResolvedValue({ status: 'ok', data: [] });

    render(InstalledModsView, { props });

    // The auto-reset must keep working once the list IS loaded — a stale link
    // must not strand the user on an empty view.
    await waitFor(() => {
      expect(document.querySelector('[data-mod-row="modrinth:PA"]')).not.toBeNull();
      expect(document.querySelector('[data-mod-row="modrinth:PB"]')).not.toBeNull();
    });
  });
});

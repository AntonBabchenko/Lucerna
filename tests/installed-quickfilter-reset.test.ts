import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// Mirrors the vi.hoisted listener registry from installed-cross-highlight.test.ts
// so the (hoisted) vi.mock factory can register the event callbacks the view
// subscribes to on mount. We fire `modInstalled` later to simulate the missing
// dependency getting installed.
const listeners = vi.hoisted(() => ({
  modInstalled: null as null | (() => void),
  modUninstalled: null as null | (() => void),
  modToggle: null as null | (() => void),
}));

// Two installed platform mods: A (sha1 'a', project PA) has a missing required
// dependency; B (sha1 'b', project PB) stands clean. The graph mock starts with
// A reporting a `missing_required` child, then is reassigned to report A clean
// after the (simulated) install. Helpers live in vi.hoisted so they exist
// before the (also hoisted) vi.mock factory runs.
const { mod, proj } = vi.hoisted(() => ({
  mod: (sha1: string, projectId: string, name: string, requires: string[] = []) => ({
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
    requires,
  }),
  proj: (projectId: string, name: string) => ({
    status: 'ok',
    data: {
      summary: {
        source: 'modrinth',
        project_id: projectId,
        slug: name.toLowerCase(),
        name,
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
}));

// Graph WITH A's missing dep: A is a root whose required child is missing →
// missingShas = {'a'}, counts.issues = 1. B is a clean root.
const graphWithMissing = vi.hoisted(() => () => ({
  status: 'ok',
  data: {
    roots: [
      {
        sha1: 'a',
        name: 'Alpha',
        required: [
          {
            source: 'modrinth',
            project_id: 'PMISSING',
            name: 'Some Lib',
            status: 'missing_required',
            cycle: false,
            children: [],
          },
        ],
        optional: [],
      },
      { sha1: 'b', name: 'Bravo', required: [], optional: [] },
    ],
  },
}));

// Graph WITHOUT the missing dep: both roots clean → missingShas empties.
const graphResolved = vi.hoisted(() => () => ({
  status: 'ok',
  data: {
    roots: [
      { sha1: 'a', name: 'Alpha', required: [], optional: [] },
      { sha1: 'b', name: 'Bravo', required: [], optional: [] },
    ],
  },
}));

const mocks = vi.hoisted(() => ({
  modsDependencyGraph: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [mod('a', 'PA', 'Alpha', ['PMISSING']), mod('b', 'PB', 'Bravo')],
    }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsProject: vi
      .fn()
      .mockImplementation((_src: string, id: string) =>
        Promise.resolve(id === 'PA' ? proj('PA', 'Alpha') : proj('PB', 'Bravo')),
      ),
    modsDependencyGraph: mocks.modsDependencyGraph,
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsDisable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsUninstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsUpdateOne: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
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

import InstalledModsView from '$lib/mods/InstalledModsView.svelte';

describe('quick-filter auto-resets to "all" when its set empties', () => {
  it("clearing the last missing dependency reveals the full list again (doesn't strand an empty filter)", async () => {
    // Start with A having a missing required dependency.
    mocks.modsDependencyGraph.mockResolvedValue(graphWithMissing());

    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });

    // Wait for the rows + the async dep-graph load. Once the graph resolves,
    // counts.issues = 1 and the toolbar's "⚠ Issues (1)" chip renders.
    const issuesChip = await waitFor(() => {
      const btn = [...document.querySelectorAll('button')].find((b) =>
        /Issues/i.test(b.textContent ?? ''),
      );
      if (!btn) throw new Error('Issues chip not rendered yet');
      return btn as HTMLButtonElement;
    });

    // Click the Issues chip → quickFilter = 'issues' → only A's row remains.
    await fireEvent.click(issuesChip);

    await waitFor(() => {
      expect(document.querySelector('[data-mod-row="modrinth:PA"]')).not.toBeNull();
      // B's row must be filtered out while the issues filter is active.
      expect(document.querySelector('[data-mod-row="modrinth:PB"]')).toBeNull();
    });

    // Simulate the dependency getting fixed: the next graph resolve reports A as
    // clean, then fire modInstalled (the shell listens → data.refresh() +
    // debounced deps.reloadGraph() which re-resolves after ~150ms).
    mocks.modsDependencyGraph.mockResolvedValue(graphResolved());
    listeners.modInstalled?.();

    // missingShas empties → counts.issues → 0 → the new reactive effect resets
    // quickFilter to 'all'. Both rows reappear (no empty-list dead-end).
    await waitFor(
      () => {
        expect(document.querySelector('[data-mod-row="modrinth:PA"]')).not.toBeNull();
        expect(document.querySelector('[data-mod-row="modrinth:PB"]')).not.toBeNull();
      },
      { timeout: 3000 },
    );
  });
});

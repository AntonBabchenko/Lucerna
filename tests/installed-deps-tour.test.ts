/**
 * The dependencies tour in the Installed list (spec 2026-10-08-deps-tour): it starts on the list
 * on screen, over a mod that requires another, opens that mod's panel from step 2, and marks only
 * the target's elements.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';
import { tourState } from '$lib/onboarding/state.svelte';
import { __resetLayers, insertTour, newLayerId } from '$lib/ui/layer-stack.svelte';

const v = vi.hoisted(() => {
  const mod = (sha1: string, projectId: string, name: string) => ({
    filename: `${name.toLowerCase()}-1.0.jar`,
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
  });
  const n = (projectId: string, name: string, installed: boolean, declared: string) => ({
    source: 'modrinth',
    project_id: projectId,
    name,
    installed,
    declared,
    cycle: false,
    children: [],
  });
  return {
    rows: [mod('a', 'PA', 'Alpha'), mod('l', 'PL', 'Lib')],
    graph: {
      roots: [
        {
          sha1: 'a',
          source: 'modrinth',
          project_id: 'PA',
          name: 'Alpha',
          required: [n('PL', 'Lib', true, 'required')],
          optional: [n('PX', 'Extra', false, 'optional')],
        },
        {
          sha1: 'l',
          source: 'modrinth',
          project_id: 'PL',
          name: 'Lib',
          required: [],
          optional: [],
        },
      ],
    },
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: v.rows }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: v.graph }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [] } }),
    modsResolveDepNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsLearnCrossIds: vi.fn().mockResolvedValue({ status: 'ok', data: { learned: 0 } }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    appSettingsGet: vi.fn(async () => ({ status: 'error', error: 'unused' })),
    appSettingsMarkTourCompleted: vi.fn(async () => ({ status: 'ok', data: null })),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

let seq = 0;
const props = (visible?: boolean) => ({
  // A fresh profile per test: the graph cache is per instance and per session.
  instanceId: `deps-tour-${++seq}`,
  mcVersion: '1.21.1',
  loader: 'fabric' as const,
  ...(visible === undefined ? {} : { visible }),
});
const card = () => document.querySelector('[data-testid="contextual-tour-popover"]');
const rowOf = (el: Element | null) => el?.closest('[data-mod-row]')?.textContent ?? '';
const listReady = async () => {
  await waitFor(() => expect(document.querySelectorAll('[data-mod-row]')).toHaveLength(2));
  await waitFor(() => expect(screen.getAllByTestId('relation-pill')).toHaveLength(2));
};
const quiet = () => new Promise((r) => setTimeout(r, 50));

describe('the dependencies tour in Installed', () => {
  beforeEach(() => {
    localStorage.clear();
    __resetLayers();
    tourState.active = false;
  });

  it('stays away while the list is not the one on screen (visible defaults to false)', async () => {
    render(InstalledModsView, { props: props() });
    await listReady();
    await quiet();
    expect(card()).toBeNull();
    expect(document.querySelector('[data-tour-ctx^="deps-"]')).toBeNull();
  });

  it('starts over the mod that requires another, and opens its panel from step 2', async () => {
    render(InstalledModsView, { props: props(true) });
    await listReady();
    await waitFor(() => expect(card()).not.toBeNull());
    expect(screen.getByText(/1 of 4/)).toBeTruthy();
    expect(rowOf(document.querySelector('[data-tour-ctx="deps-cell"]'))).toContain('Alpha');
    expect(rowOf(document.querySelector('[data-tour-ctx="deps-required-by"]'))).toContain('Lib');
    expect(document.querySelector('[data-dep-section]')).toBeNull();

    const next = document.querySelector<HTMLElement>('[data-tour-primary]');
    if (!next) throw new Error('no tour primary button on screen');
    await fireEvent.click(next);
    await waitFor(() =>
      expect(document.querySelector('[data-tour-ctx="deps-requires"]')).not.toBeNull(),
    );
    expect(document.querySelector('[data-tour-ctx="deps-optional"]')).not.toBeNull();
    expect(screen.getByText(/2 of 4/)).toBeTruthy();
  });

  it('opens nothing for a tour already seen', async () => {
    localStorage.setItem('ftl.tour.deps.v1.done', '1');
    render(InstalledModsView, { props: props(true) });
    await listReady();
    await quiet();
    expect(card()).toBeNull();
    expect(document.querySelector('[data-dep-section]')).toBeNull();
  });

  it('waits for another tour and starts once it ends', async () => {
    const release = insertTour(newLayerId('addons-like'), null, () => {});
    render(InstalledModsView, { props: props(true) });
    await listReady();
    await quiet();
    expect(card()).toBeNull();
    release?.();
    await waitFor(() => expect(card()).not.toBeNull());
  });
});

/**
 * A jar built for another Minecraft or loader version needs another build of itself (spec §6.2).
 * Its row offers «Choose version» — the mod's own version list, in the detail modal — and its
 * «What stops the game» row offers the same (plan §5b V1, screenshots 03 vs 01): the panel row was
 * a dead end.
 */
import { fireEvent, render, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({
  row: {
    filename: 'btp.jar',
    sha1: 'btp',
    source: 'modrinth',
    project_id: 'PBTP',
    version_id: 'v-btp',
    name: 'Better Third Person',
    version_number: '1.9.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
    requires: [],
  },
  // Made for Minecraft 1.20.1–1.21; the profile runs 1.21.1.
  mismatch: {
    dependent_sha1: 'btp',
    dependent_name: 'Better Third Person',
    dep_id: 'minecraft',
    kind: 'platform_mismatch',
    installed_version: '1.21.1',
    needed: '[1.20.1,1.21)',
    needed_desc: {
      raw: '[1.20.1,1.21)',
      family: 'maven',
      alternatives: [],
      unparseable: true,
      soft: false,
    },
    provider_project: null,
    provider_sha1: null,
    family: 'maven',
  },
  modsProject: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [h.row] }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    instanceDependencyPreflight: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { violations: [h.mismatch] } }),
    modsResolveDepNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // The detail modal loads the project it shows first: which project was opened.
    modsProject: h.modsProject,
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

describe('a platform mismatch in «What stops the game»', () => {
  it('offers the row’s «Choose version», which opens the mod’s own version list', async () => {
    h.modsProject.mockResolvedValue({ status: 'error', error: { kind: 'network' } });
    render(InstalledModsView, {
      props: { instanceId: 'platform-choose', mcVersion: '1.21.1', loader: 'fabric' as const },
    });
    const row = await waitFor(() => {
      const el = document.querySelector<HTMLElement>('[data-testid="preflight-row"]');
      if (!el) throw new Error('panel row not rendered yet');
      return el;
    });
    // Offered once the list says the mod has a platform identity to list builds from.
    await fireEvent.click(await within(row).findByRole('button', { name: 'Choose version' }));
    await waitFor(() => expect(h.modsProject).toHaveBeenCalledWith('modrinth', 'PBTP'));
  });
});

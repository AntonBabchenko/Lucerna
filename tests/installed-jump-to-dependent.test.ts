/**
 * «Show in the list» on a «What stops the game» row jumps to the mod that has the problem. When a search or a
 * chip hides it, they are cleared and the row is shown. When the report predates a removal the
 * mod is in no view at all: the jump says so and leaves the filters alone — clearing them would
 * change the list for nothing and still show nothing.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({
  alpha: {
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
  },
  instanceDependencyPreflight: vi.fn(),
  pushInfo: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [h.alpha] }),
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
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
  pushActionToast: vi.fn(),
  pushInfo: h.pushInfo,
}));

import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

const missing = (dependentSha1: string, dependentName: string) => ({
  dependent_sha1: dependentSha1,
  dependent_name: dependentName,
  dep_id: 'lib',
  kind: 'missing_required',
  installed_version: null,
  needed: '',
  needed_desc: { raw: '', family: 'maven', alternatives: [], unparseable: false, soft: false },
  provider_project: null,
  provider_sha1: null,
  family: null,
});

// A DISTINCT instance id per case: the pre-flight cache is a per-instance LRU.
const props = (instanceId: string) => ({
  instanceId,
  mcVersion: '1.21.1',
  loader: 'neoforge' as const,
});
const search = () => screen.getByRole('searchbox', { name: 'Filter installed mods' });

beforeEach(() => {
  h.instanceDependencyPreflight.mockReset();
  h.pushInfo.mockReset();
});

describe('«show in the list» to the mod a «What stops the game» row is about', () => {
  it('clears the search that hides it and shows its row', async () => {
    h.instanceDependencyPreflight.mockResolvedValue({
      status: 'ok',
      data: { violations: [missing('a', 'Alpha')] },
    });
    render(InstalledModsView, { props: props('jump-hidden') });
    const jump = await screen.findByRole('button', { name: 'Show Alpha in the list' });
    await fireEvent.input(search(), { target: { value: 'zzz' } });
    await waitFor(() => expect(document.querySelector('[data-mod-row]')).toBeNull());

    // Shown = back in the list and scrolled into view; nothing marks the row beyond that.
    const scrolled = vi.spyOn(Element.prototype, 'scrollIntoView');
    try {
      await fireEvent.click(jump);

      await waitFor(() => {
        const row = document.querySelector('[data-mod-row="modrinth:PA"]');
        expect(row).not.toBeNull();
        expect(scrolled.mock.contexts).toContain(row);
      });
    } finally {
      scrolled.mockRestore();
    }
    expect((search() as HTMLInputElement).value).toBe('');
    expect(h.pushInfo).not.toHaveBeenCalled();
  });

  it('says a mod removed since the report is no longer installed, and changes no filter', async () => {
    h.instanceDependencyPreflight.mockResolvedValue({
      status: 'ok',
      data: { violations: [missing('gone', 'Gone Mod')] },
    });
    render(InstalledModsView, { props: props('jump-gone') });
    const jump = await screen.findByRole('button', { name: 'Show Gone Mod in the list' });
    await fireEvent.input(search(), { target: { value: 'alp' } });

    await fireEvent.click(jump);

    await waitFor(() => expect(h.pushInfo).toHaveBeenCalledWith('Gone Mod is no longer installed'));
    expect((search() as HTMLInputElement).value).toBe('alp');
    expect(document.querySelector('[data-mod-row="modrinth:PA"]')).not.toBeNull();
  });
});

import { render } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  initialBrowse,
  rememberBrowse,
  resetAddonsViewMemory,
} from '$lib/mods/addons-view-memory.svelte';
import ModBrowseView from '$lib/mods/ModBrowseView.svelte';

const search = vi.hoisted(() =>
  vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
);

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsSearch: search,
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    assetsList: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    modInstalled: { listen: vi.fn().mockResolvedValue(() => {}) },
    modUninstalled: { listen: vi.fn().mockResolvedValue(() => {}) },
    modToggle: { listen: vi.fn().mockResolvedValue(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

const settle = () => new Promise((r) => setTimeout(r, 0));

afterEach(() => {
  resetAddonsViewMemory();
  search.mockClear();
});

describe('the Browse catalogue remembers what was searched', () => {
  it('per kind, for the session, with a plain default', () => {
    expect(initialBrowse('mod')).toEqual({ query: '', sort: 'downloads' });
    rememberBrowse('mod', { query: 'sodium', sort: 'updated' });
    expect(initialBrowse('mod')).toEqual({ query: 'sodium', sort: 'updated' });
    expect(initialBrowse('shader')).toEqual({ query: '', sort: 'downloads' });
    resetAddonsViewMemory();
    expect(initialBrowse('mod')).toEqual({ query: '', sort: 'downloads' });
  });

  // The Settings tab replaced a dialog that opened OVER Add-ons: leaving for it
  // now unmounts the catalogue, and coming back must not start from scratch.
  it('a fresh catalogue starts from the remembered search', async () => {
    rememberBrowse('mod', { query: 'sodium', sort: 'updated' });
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await settle();
    expect(search).toHaveBeenCalled();
    const req = search.mock.calls[0][0];
    expect(req.query).toBe('sodium');
    expect(req.sort).toBe('updated');
  });

  it('what is typed is remembered for the next mount', async () => {
    const { unmount, container } = render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const input = container.querySelector(
      'input[type="search"], input[type="text"]',
    ) as HTMLInputElement;
    input.value = 'lithium';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
    unmount();
    expect(initialBrowse('mod').query).toBe('lithium');
  });
});

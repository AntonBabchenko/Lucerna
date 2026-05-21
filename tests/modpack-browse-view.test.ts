import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// `vi.mock` factory is hoisted above the SUT import, so any variable it
// captures must be declared via `vi.hoisted` (also hoisted). `modpack_search`
// returns the tauri-specta `{ status, data | error }` shape.
const { mockSearch, mockKeyStatus } = vi.hoisted(() => ({
  mockSearch: vi.fn().mockResolvedValue({
    status: 'ok',
    data: { hits: [], total: 0, offset: 0, limit: 20 },
  }),
  mockKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'present' }),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: { modpackSearch: mockSearch, modsGetCurseforgeKeyStatus: mockKeyStatus },
  events: {},
}));
vi.mock('$lib/settings/state.svelte', () => ({
  cfKeyVersion: { value: 0 },
  settingsOpen: { value: null },
}));

import ModpackBrowseView from '$lib/modpacks/ModpackBrowseView.svelte';

// Svelte 5 `bind:value` on <select> reads `select.querySelector(':checked')`,
// but happy-dom's `:checked` only matches INPUT elements. Patch the
// querySelector to fall back to the actually-selected option.
// Same workaround as `tests/imported-view.test.ts`.
function changeSelect(el: HTMLSelectElement, value: string) {
  const orig = el.querySelector.bind(el);
  el.querySelector = (sel: string) => {
    if (sel === ':checked') {
      return Array.from(el.options).find((o) => o.selected) ?? null;
    }
    return orig(sel);
  };
  el.value = value;
  return fireEvent.change(el);
}

describe('ModpackBrowseView', () => {
  beforeEach(() => {
    mockSearch.mockClear();
    mockKeyStatus.mockClear();
    mockKeyStatus.mockResolvedValue({ status: 'ok', data: 'present' });
  });

  it('renders the source picker', () => {
    const { getByLabelText } = render(ModpackBrowseView, {
      props: { onPickHit: () => {} },
    });
    expect(getByLabelText('Mod source')).toBeTruthy();
  });

  it('switching source to CurseForge with no key shows the key banner', async () => {
    mockKeyStatus.mockResolvedValue({ status: 'ok', data: 'missing' });
    const { getByLabelText, findByText } = render(ModpackBrowseView, {
      props: { onPickHit: () => {} },
    });
    await changeSelect(getByLabelText('Mod source') as HTMLSelectElement, 'curseforge');
    expect(await findByText('CurseForge requires an API key')).toBeTruthy();
  });

  it('renders all four toolbar controls', () => {
    const { getByTestId } = render(ModpackBrowseView, {
      props: { onPickHit: () => {} },
    });
    expect(getByTestId('modpack-search-input')).toBeTruthy();
    expect(getByTestId('modpack-mc-input')).toBeTruthy();
    expect(getByTestId('modpack-loader-select')).toBeTruthy();
    expect(getByTestId('modpack-sort-select')).toBeTruthy();
  });

  it('initial search uses relevance + null filters', async () => {
    render(ModpackBrowseView, { props: { onPickHit: () => {} } });
    // initial mount triggers a debounced search; wait for it to land
    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    const args = mockSearch.mock.calls.at(-1) ?? [];
    expect(args[0]).toBe('modrinth'); // source
    expect(args[3]).toBeNull(); // mc
    expect(args[4]).toBeNull(); // loader
    expect(args[5]).toBe('relevance'); // sort
  });

  it('typing MC version triggers search with mc_version param', async () => {
    const { getByTestId } = render(ModpackBrowseView, {
      props: { onPickHit: () => {} },
    });
    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    mockSearch.mockClear();

    const mcInput = getByTestId('modpack-mc-input') as HTMLInputElement;
    mcInput.value = '1.20.1';
    await fireEvent.input(mcInput);

    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    const args = mockSearch.mock.calls.at(-1) ?? [];
    expect(args[3]).toBe('1.20.1');
  });

  it('changing sort triggers search with sort param', async () => {
    const { getByTestId } = render(ModpackBrowseView, {
      props: { onPickHit: () => {} },
    });
    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    mockSearch.mockClear();

    const sortSelect = getByTestId('modpack-sort-select') as HTMLSelectElement;
    await changeSelect(sortSelect, 'downloads');

    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    const args = mockSearch.mock.calls.at(-1) ?? [];
    expect(args[5]).toBe('downloads');
  });

  it('selecting loader triggers search with loader param', async () => {
    const { getByTestId } = render(ModpackBrowseView, {
      props: { onPickHit: () => {} },
    });
    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    mockSearch.mockClear();

    const loaderSelect = getByTestId('modpack-loader-select') as HTMLSelectElement;
    await changeSelect(loaderSelect, 'fabric');

    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    const args = mockSearch.mock.calls.at(-1) ?? [];
    expect(args[4]).toBe('fabric');
  });
});

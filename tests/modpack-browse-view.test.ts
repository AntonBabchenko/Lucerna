import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// `vi.mock` factory is hoisted above the SUT import, so any variable it
// captures must be declared via `vi.hoisted` (also hoisted). `modpack_search`
// returns the tauri-specta `{ status, data | error }` shape.
const { mockSearch, mockKeyStatus, mockSourceCaps } = vi.hoisted(() => ({
  mockSearch: vi.fn().mockResolvedValue({
    status: 'ok',
    data: { hits: [], total: 0, offset: 0, limit: 20 },
  }),
  mockKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'present' }),
  // Returns realistic caps per source: CurseForge needs_api_key, FTB no
  // server filters, Modrinth defaults (all false/true).
  mockSourceCaps: vi.fn().mockImplementation(async (source: string) => ({
    status: 'ok',
    data: {
      needs_api_key: source === 'curseforge',
      supports_server_filter: source !== 'ftb',
      can_export: source !== 'ftb',
    },
  })),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpackSearch: mockSearch,
    modsGetCurseforgeKeyStatus: mockKeyStatus,
    modpackSourceCaps: mockSourceCaps,
  },
  events: {},
}));
vi.mock('$lib/settings/state.svelte', () => ({
  cfKeyVersion: { value: 0 },
  settingsOpen: { value: null },
  // McVersionCombobox reads from this rune. Empty list is fine — the
  // combobox just shows the "Any version" row and the test types into
  // the input directly.
  mcVersions: { value: [] },
}));

import ModpackBrowseView from '$lib/modpacks/ModpackBrowseView.svelte';

// The sort control is now the custom <Select> listbox: open the trigger by
// testid, then commit an option by its accessible name. The Select commits on
// `mousedown` (not click), so the option must be driven with mouseDown.
async function pickOption(triggerTestid: string, name: RegExp | string) {
  await fireEvent.click(screen.getByTestId(triggerTestid));
  await fireEvent.mouseDown(screen.getByRole('option', { name }));
}

describe('ModpackBrowseView', () => {
  beforeEach(() => {
    mockSearch.mockClear();
    mockKeyStatus.mockClear();
    mockKeyStatus.mockResolvedValue({ status: 'ok', data: 'present' });
    mockSourceCaps.mockClear();
    mockSourceCaps.mockImplementation(async (source: string) => ({
      status: 'ok',
      data: {
        needs_api_key: source === 'curseforge',
        supports_server_filter: source !== 'ftb',
        can_export: source !== 'ftb',
      },
    }));
  });

  it('renders the source segmented control inside the drawer', async () => {
    const { getByTestId } = render(ModpackBrowseView, { props: { onPickHit: () => {} } });
    await fireEvent.click(getByTestId('browse-filters-button'));
    expect(screen.getByRole('radiogroup', { name: /mod source/i })).toBeTruthy();
  });

  it('switching source to CurseForge with no key shows the key banner', async () => {
    mockKeyStatus.mockResolvedValue({ status: 'ok', data: 'missing' });
    const { getByTestId, findByText } = render(ModpackBrowseView, {
      props: { onPickHit: () => {} },
    });
    await fireEvent.click(getByTestId('browse-filters-button'));
    await fireEvent.click(screen.getByRole('radio', { name: 'CurseForge' }));
    expect(await findByText('CurseForge requires an API key')).toBeTruthy();
  });

  it('renders the core toolbar controls', () => {
    const { getByTestId } = render(ModpackBrowseView, { props: { onPickHit: () => {} } });
    expect(getByTestId('modpack-search-input')).toBeTruthy();
    expect(getByTestId('modpack-sort-select')).toBeTruthy();
    expect(getByTestId('browse-filters-button')).toBeTruthy();
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
    const { getByTestId } = render(ModpackBrowseView, { props: { onPickHit: () => {} } });
    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    mockSearch.mockClear();
    await fireEvent.click(getByTestId('browse-filters-button'));
    const mcInput = getByTestId('modpack-mc-input') as HTMLInputElement;
    mcInput.value = '1.20.1';
    await fireEvent.input(mcInput);
    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    const args = mockSearch.mock.calls.at(-1) ?? [];
    expect(args[3]).toBe('1.20.1');
  });

  it('changing sort triggers search with sort param', async () => {
    render(ModpackBrowseView, {
      props: { onPickHit: () => {} },
    });
    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    mockSearch.mockClear();

    await pickOption('modpack-sort-select', /downloads/i);

    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    const args = mockSearch.mock.calls.at(-1) ?? [];
    expect(args[5]).toBe('downloads');
  });

  it('selecting loader triggers search with loader param', async () => {
    const { getByTestId } = render(ModpackBrowseView, { props: { onPickHit: () => {} } });
    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    mockSearch.mockClear();
    await fireEvent.click(getByTestId('browse-filters-button'));
    await fireEvent.click(screen.getByRole('radio', { name: 'Fabric' }));
    await waitFor(() => expect(mockSearch).toHaveBeenCalled(), { timeout: 1000 });
    const args = mockSearch.mock.calls.at(-1) ?? [];
    expect(args[4]).toBe('fabric');
  });
});

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ShareExportDialog from '$lib/l10n/ShareExportDialog.svelte';

const mocks = vi.hoisted(() => ({
  l10nOverriddenNamespaces: vi.fn(),
  l10nExport: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));

const saveMock = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: saveMock }));

const toasts = vi.hoisted(() => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
  pushInfo: vi.fn(),
}));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);

beforeEach(() => {
  vi.clearAllMocks();
});

/** `mod00`… — enough rows to cross the dialog's search threshold. */
const manyNamespaces = (n: number) =>
  Array.from({ length: n }, (_, i) => `mod${String(i).padStart(2, '0')}`);

const props = (over: Record<string, unknown> = {}) => ({
  lang: 'ru_ru',
  mcVersion: '1.21.1',
  instanceNamespaces: ['create'],
  onClose: vi.fn(),
  ...over,
});

describe('ShareExportDialog', () => {
  it('lists every stored namespace but pre-ticks only this instance mods', async () => {
    mocks.l10nOverriddenNamespaces.mockResolvedValue({ status: 'ok', data: ['ae2', 'create'] });
    render(ShareExportDialog, { props: props() });

    await waitFor(() => screen.getByTestId('share-export-ns-create'));
    expect((screen.getByTestId('share-export-ns-create') as HTMLInputElement).checked).toBe(true);
    expect((screen.getByTestId('share-export-ns-ae2') as HTMLInputElement).checked).toBe(false);
  });

  it('disables Export when the selection is emptied', async () => {
    mocks.l10nOverriddenNamespaces.mockResolvedValue({ status: 'ok', data: ['create'] });
    render(ShareExportDialog, { props: props() });

    await waitFor(() => screen.getByTestId('share-export-ns-create'));
    expect((screen.getByTestId('share-export-run') as HTMLButtonElement).disabled).toBe(false);
    await fireEvent.click(screen.getByTestId('share-export-ns-create'));
    expect((screen.getByTestId('share-export-run') as HTMLButtonElement).disabled).toBe(true);
  });

  it('passes version, selection, note and the chosen path to the command', async () => {
    mocks.l10nOverriddenNamespaces.mockResolvedValue({ status: 'ok', data: ['create'] });
    saveMock.mockResolvedValue('C:/tmp/lucerna-translations-ru_ru-2026-08-04.zip');
    mocks.l10nExport.mockResolvedValue({ status: 'ok', data: null });
    render(ShareExportDialog, { props: props() });

    await waitFor(() => screen.getByTestId('share-export-run'));
    await fireEvent.input(screen.getByTestId('share-export-note'), {
      target: { value: 'от меня' },
    });
    await fireEvent.click(screen.getByTestId('share-export-run'));

    await waitFor(() =>
      expect(mocks.l10nExport).toHaveBeenCalledWith(
        '1.21.1',
        'ru_ru',
        ['create'],
        'от меня',
        'C:/tmp/lucerna-translations-ru_ru-2026-08-04.zip',
      ),
    );
  });

  it('names what the list is and what a tick means', async () => {
    mocks.l10nOverriddenNamespaces.mockResolvedValue({ status: 'ok', data: ['ae2', 'create'] });
    render(ShareExportDialog, { props: props() });

    await waitFor(() => screen.getByTestId('share-export-ns-create'));
    expect(screen.getByTestId('share-export-heading')).toBeTruthy();
    expect(screen.getByTestId('share-export-tick-note')).toBeTruthy();
  });

  it('explains an empty store instead of showing a bare box', async () => {
    mocks.l10nOverriddenNamespaces.mockResolvedValue({ status: 'ok', data: [] });
    render(ShareExportDialog, { props: props() });

    await waitFor(() => screen.getByTestId('share-export-empty'));
    // The note field describes a file that cannot be produced, so it goes too.
    expect(screen.queryByTestId('share-export-note')).toBeNull();
    expect((screen.getByTestId('share-export-run') as HTMLButtonElement).disabled).toBe(true);
  });

  it('counts the selection against the whole store', async () => {
    mocks.l10nOverriddenNamespaces.mockResolvedValue({ status: 'ok', data: ['ae2', 'create'] });
    render(ShareExportDialog, { props: props() });

    await waitFor(() => screen.getByTestId('share-export-count'));
    const count = () => screen.getByTestId('share-export-count');
    expect(count().dataset.selected).toBe('1');
    expect(count().dataset.total).toBe('2');

    await fireEvent.click(screen.getByTestId('share-export-ns-ae2'));
    expect(count().dataset.selected).toBe('2');
  });

  it('selects and clears in bulk', async () => {
    mocks.l10nOverriddenNamespaces.mockResolvedValue({ status: 'ok', data: ['ae2', 'create'] });
    render(ShareExportDialog, { props: props() });

    await waitFor(() => screen.getByTestId('share-export-select-all'));
    await fireEvent.click(screen.getByTestId('share-export-select-all'));
    expect(screen.getByTestId('share-export-count').dataset.selected).toBe('2');

    await fireEvent.click(screen.getByTestId('share-export-clear'));
    expect(screen.getByTestId('share-export-count').dataset.selected).toBe('0');
    expect((screen.getByTestId('share-export-run') as HTMLButtonElement).disabled).toBe(true);
  });

  it('offers search only once the list is long enough to need it', async () => {
    mocks.l10nOverriddenNamespaces.mockResolvedValue({ status: 'ok', data: manyNamespaces(10) });
    const short = render(ShareExportDialog, { props: props() });
    await waitFor(() => screen.getByTestId('share-export-ns-mod00'));
    expect(screen.queryByTestId('share-export-search')).toBeNull();
    short.unmount();

    mocks.l10nOverriddenNamespaces.mockResolvedValue({ status: 'ok', data: manyNamespaces(11) });
    render(ShareExportDialog, { props: props() });
    await waitFor(() => screen.getByTestId('share-export-search'));
  });

  it('hides non-matching rows while searching', async () => {
    mocks.l10nOverriddenNamespaces.mockResolvedValue({
      status: 'ok',
      data: ['create', ...manyNamespaces(10)],
    });
    render(ShareExportDialog, { props: props() });

    await waitFor(() => screen.getByTestId('share-export-search'));
    await fireEvent.input(screen.getByTestId('share-export-search'), {
      target: { value: 'crea' },
    });

    expect(screen.getByTestId('share-export-ns-create')).toBeTruthy();
    expect(screen.queryByTestId('share-export-ns-mod00')).toBeNull();
  });

  it('exports a ticked namespace the filter is currently hiding', async () => {
    // The whole point of the filter being a view and not a mutation. Exporting
    // only what happens to be visible would silently drop the rest.
    mocks.l10nOverriddenNamespaces.mockResolvedValue({
      status: 'ok',
      data: ['create', ...manyNamespaces(10)],
    });
    saveMock.mockResolvedValue('C:/tmp/out.zip');
    mocks.l10nExport.mockResolvedValue({ status: 'ok', data: null });
    render(ShareExportDialog, { props: props() });

    await waitFor(() => screen.getByTestId('share-export-search'));
    await fireEvent.input(screen.getByTestId('share-export-search'), {
      target: { value: 'mod07' },
    });
    expect(screen.queryByTestId('share-export-ns-create')).toBeNull();

    await fireEvent.click(screen.getByTestId('share-export-run'));

    await waitFor(() =>
      expect(mocks.l10nExport).toHaveBeenCalledWith(
        '1.21.1',
        'ru_ru',
        ['create'],
        '',
        'C:/tmp/out.zip',
      ),
    );
  });

  it('selects only the matching rows when a filter is active', async () => {
    mocks.l10nOverriddenNamespaces.mockResolvedValue({
      status: 'ok',
      data: ['create', ...manyNamespaces(10)],
    });
    render(ShareExportDialog, { props: props({ instanceNamespaces: [] }) });

    await waitFor(() => screen.getByTestId('share-export-search'));
    await fireEvent.input(screen.getByTestId('share-export-search'), {
      target: { value: 'mod01' },
    });
    await fireEvent.click(screen.getByTestId('share-export-select-all'));

    expect(screen.getByTestId('share-export-count').dataset.selected).toBe('1');

    await fireEvent.input(screen.getByTestId('share-export-search'), { target: { value: '' } });
    expect((screen.getByTestId('share-export-ns-mod01') as HTMLInputElement).checked).toBe(true);
    expect((screen.getByTestId('share-export-ns-create') as HTMLInputElement).checked).toBe(false);
  });

  it('does nothing when the user cancels the save dialog', async () => {
    mocks.l10nOverriddenNamespaces.mockResolvedValue({ status: 'ok', data: ['create'] });
    saveMock.mockResolvedValue(null);
    const onClose = vi.fn();
    render(ShareExportDialog, { props: props({ onClose }) });

    await waitFor(() => screen.getByTestId('share-export-run'));
    await fireEvent.click(screen.getByTestId('share-export-run'));

    await waitFor(() => expect(saveMock).toHaveBeenCalled());
    expect(mocks.l10nExport).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });
});

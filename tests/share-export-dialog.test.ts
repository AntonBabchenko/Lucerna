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

const toasts = vi.hoisted(() => ({ pushSuccess: vi.fn(), pushWarning: vi.fn(), pushInfo: vi.fn() }));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);

beforeEach(() => {
  vi.clearAllMocks();
});

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

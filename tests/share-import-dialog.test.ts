// Receiving a friend's translations. The file is a stranger's zip, so what is
// worth pinning here is not layout but the promises the dialog makes about it:
//
//   - the user sees what an import WOULD do before anything is written;
//   - a file for a different language says so, because the entries land in
//     THAT language rather than the one open in the editor;
//   - the conflict default is "keep mine" — importing never silently
//     overwrites the user's own work, taking the sender's is a choice;
//   - a bundle with nothing importable cannot be imported;
//   - the parent is told the BUNDLE's language, not the prop, because it
//     decides what to refresh and where to offer applying it;
//   - a plain resource pack gets an explanation, not a bare failure.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
// Static import: a dynamic `await import('*.svelte')` inside a test body never
// resolves under this vitest setup.
import ShareImportDialog from '$lib/l10n/ShareImportDialog.svelte';

const mocks = vi.hoisted(() => ({
  l10nInspectBundle: vi.fn(),
  l10nImportBundle: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: mocks }));

const openMock = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: openMock }));

const summary = (over: Record<string, unknown> = {}) => ({
  lang: 'uk_ua',
  note: 'от друга',
  newEntries: 5,
  conflicts: 2,
  invalid: 1,
  namespaces: ['create'],
  ...over,
});

const props = (over: Record<string, unknown> = {}) => ({
  lang: 'ru_ru',
  onImported: vi.fn(),
  onClose: vi.fn(),
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
});

describe('ShareImportDialog', () => {
  it('shows the summary, announces a cross-language import and defaults to keep-mine', async () => {
    openMock.mockResolvedValue('C:/tmp/b.zip');
    mocks.l10nInspectBundle.mockResolvedValue({ status: 'ok', data: summary() });
    mocks.l10nImportBundle.mockResolvedValue({
      status: 'ok',
      data: { added: 5, replaced: 0, kept: 2, invalid: 1, lang: 'uk_ua' },
    });
    render(ShareImportDialog, { props: props() });

    await waitFor(() => screen.getByTestId('share-import-run'));
    expect(screen.getByTestId('share-import-summary').textContent).toContain('5');
    expect(screen.getByTestId('share-import-crosslang').textContent).toContain('uk_ua');

    await fireEvent.click(screen.getByTestId('share-import-run'));
    await waitFor(() =>
      expect(mocks.l10nImportBundle).toHaveBeenCalledWith('C:/tmp/b.zip', 'keep_mine'),
    );
  });

  it('sends take_file once the user chooses it', async () => {
    openMock.mockResolvedValue('C:/tmp/b.zip');
    mocks.l10nInspectBundle.mockResolvedValue({ status: 'ok', data: summary() });
    mocks.l10nImportBundle.mockResolvedValue({
      status: 'ok',
      data: { added: 5, replaced: 2, kept: 0, invalid: 1, lang: 'uk_ua' },
    });
    render(ShareImportDialog, { props: props() });

    await waitFor(() => screen.getByTestId('share-import-take'));
    await fireEvent.click(screen.getByTestId('share-import-take'));
    await fireEvent.click(screen.getByTestId('share-import-run'));

    await waitFor(() =>
      expect(mocks.l10nImportBundle).toHaveBeenCalledWith('C:/tmp/b.zip', 'take_file'),
    );
  });

  it('reports the bundle language to the parent, not the language prop', async () => {
    const onImported = vi.fn();
    openMock.mockResolvedValue('C:/tmp/b.zip');
    mocks.l10nInspectBundle.mockResolvedValue({ status: 'ok', data: summary() });
    mocks.l10nImportBundle.mockResolvedValue({
      status: 'ok',
      data: { added: 5, replaced: 0, kept: 2, invalid: 1, lang: 'uk_ua' },
    });
    render(ShareImportDialog, { props: props({ onImported }) });

    await waitFor(() => screen.getByTestId('share-import-run'));
    await fireEvent.click(screen.getByTestId('share-import-run'));

    await waitFor(() => expect(onImported).toHaveBeenCalledWith({ lang: 'uk_ua' }));
  });

  it('disables Import when nothing is importable', async () => {
    openMock.mockResolvedValue('C:/tmp/b.zip');
    mocks.l10nInspectBundle.mockResolvedValue({
      status: 'ok',
      data: summary({ newEntries: 0, conflicts: 0, invalid: 7 }),
    });
    render(ShareImportDialog, { props: props() });

    await waitFor(() => screen.getByTestId('share-import-run'));
    expect((screen.getByTestId('share-import-run') as HTMLButtonElement).disabled).toBe(true);
  });

  it('closes when the file picker is cancelled', async () => {
    const onClose = vi.fn();
    openMock.mockResolvedValue(null);
    render(ShareImportDialog, { props: props({ onClose }) });

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(mocks.l10nInspectBundle).not.toHaveBeenCalled();
  });

  it('explains a plain resource pack instead of a bare failure', async () => {
    openMock.mockResolvedValue('C:/tmp/pack.zip');
    mocks.l10nInspectBundle.mockResolvedValue({
      status: 'error',
      error: {
        kind: 'l10n_share_bundle_invalid',
        error: { kind: 'no_metadata', looks_like_resource_pack: true },
      },
    });
    render(ShareImportDialog, { props: props() });

    await waitFor(() => expect(screen.getByTestId('share-import-summary')).toBeTruthy());
    const text = screen.getByTestId('share-import-summary').textContent ?? '';
    expect(text.length).toBeGreaterThan(0);
    expect(mocks.l10nImportBundle).not.toHaveBeenCalled();
  });
});

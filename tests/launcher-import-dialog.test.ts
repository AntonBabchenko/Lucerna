import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// Mock plugin-dialog before SUT import (vitest hoists mocks).
const openFileMock = vi.fn();
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: (...args: unknown[]) => openFileMock(...args),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    launcherImportDiscover: vi.fn(),
    launcherImportInspectFolder: vi.fn(),
  },
}));

vi.mock('$lib/ops/op-queue.svelte', () => ({
  enqueueLauncherImport: vi.fn(),
}));

vi.mock('$lib/ipc/format-error', () => ({
  formatError: vi.fn((e: { kind: string }) => `err:${e.kind}`),
}));

vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (run: (v: (k: string, p?: Record<string, unknown>) => string) => void) => {
      run((k: string) => k);
      return () => {};
    },
  },
}));

import LauncherImportDialog from '$lib/instances/import/LauncherImportDialog.svelte';
import { commands } from '$lib/ipc/bindings';
import { enqueueLauncherImport } from '$lib/ops/op-queue.svelte';

const mockForeign = {
  source: 'prism' as const,
  name: 'My Pack',
  root: '/prism/instances/my-pack',
  minecraft_dir: '/prism/instances/my-pack/.minecraft',
  mc_version: '1.20.4',
  loader: 'fabric' as const,
  loader_version: '0.15.7',
  max_heap_mb: 4096,
  extra_jvm_args: null,
  content: [
    { category: 'mods' as const, file_count: 42, total_bytes: 104857600 },
    { category: 'config' as const, file_count: 5, total_bytes: null },
  ],
  known_mods: [],
};

describe('LauncherImportDialog', () => {
  // The dialog auto-scans on mount; give every test a safe default so the
  // mount-time discover() never sees an undefined result. Tests that care
  // about the result override this in their body before rendering.
  beforeEach(() => {
    (commands.launcherImportDiscover as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [],
    });
  });

  it('renders step 1 with discover and browse buttons', () => {
    const { getByTestId } = render(LauncherImportDialog, {
      props: { onClose: vi.fn() },
    });
    expect(getByTestId('discover-btn')).toBeTruthy();
    expect(getByTestId('browse-folder-btn')).toBeTruthy();
  });

  it('calls launcherImportDiscover and shows results', async () => {
    (commands.launcherImportDiscover as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [mockForeign],
    });

    const { getByTestId } = render(LauncherImportDialog, {
      props: { onClose: vi.fn() },
    });

    fireEvent.click(getByTestId('discover-btn'));
    await waitFor(() => expect(getByTestId('discovered-list')).toBeTruthy());

    const rows = getByTestId('discovered-list').querySelectorAll('[data-testid="instance-row"]');
    expect(rows).toHaveLength(1);
    expect(rows[0].textContent).toContain('My Pack');
  });

  it('shows empty state when discovery returns no instances', async () => {
    (commands.launcherImportDiscover as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [],
    });

    const { getByTestId } = render(LauncherImportDialog, {
      props: { onClose: vi.fn() },
    });

    fireEvent.click(getByTestId('discover-btn'));
    await waitFor(() => expect(getByTestId('discover-empty')).toBeTruthy());
  });

  it('shows error when discovery fails', async () => {
    (commands.launcherImportDiscover as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: '/x', details: 'nope' },
    });

    const { getByTestId } = render(LauncherImportDialog, {
      props: { onClose: vi.fn() },
    });

    fireEvent.click(getByTestId('discover-btn'));
    await waitFor(() => {
      const el = getByTestId('discover-error');
      expect(el.textContent).toContain('err:io');
    });
  });

  it('clicking an instance row advances to step 2', async () => {
    (commands.launcherImportDiscover as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [mockForeign],
    });

    const { getByTestId } = render(LauncherImportDialog, {
      props: { onClose: vi.fn() },
    });

    fireEvent.click(getByTestId('discover-btn'));
    await waitFor(() => expect(getByTestId('discovered-list')).toBeTruthy());

    const row = getByTestId('discovered-list').querySelector(
      '[data-testid="instance-row"]',
    ) as Element;
    fireEvent.click(row);

    await waitFor(() => expect(getByTestId('import-btn')).toBeTruthy());
    expect((getByTestId('name-input') as HTMLInputElement).value).toBe('My Pack');
  });

  it('step 2: categories are pre-checked and toggleable', async () => {
    (commands.launcherImportDiscover as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [mockForeign],
    });

    const { getByTestId } = render(LauncherImportDialog, {
      props: { onClose: vi.fn() },
    });

    fireEvent.click(getByTestId('discover-btn'));
    await waitFor(() => expect(getByTestId('discovered-list')).toBeTruthy());
    fireEvent.click(
      getByTestId('discovered-list').querySelector('[data-testid="instance-row"]') as Element,
    );
    await waitFor(() => expect(getByTestId('category-list')).toBeTruthy());

    const modsCb = getByTestId('cat-mods') as HTMLInputElement;
    expect(modsCb.checked).toBe(true);

    fireEvent.click(modsCb);
    await waitFor(() => expect((getByTestId('cat-mods') as HTMLInputElement).checked).toBe(false));
  });

  it('step 2: import button calls enqueueLauncherImport and closes dialog', async () => {
    (commands.launcherImportDiscover as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [mockForeign],
    });
    const onClose = vi.fn();

    const { getByTestId } = render(LauncherImportDialog, {
      props: { onClose },
    });

    fireEvent.click(getByTestId('discover-btn'));
    await waitFor(() => expect(getByTestId('discovered-list')).toBeTruthy());
    fireEvent.click(
      getByTestId('discovered-list').querySelector('[data-testid="instance-row"]') as Element,
    );
    await waitFor(() => expect(getByTestId('import-btn')).toBeTruthy());

    fireEvent.click(getByTestId('import-btn'));
    await waitFor(() => expect(enqueueLauncherImport).toHaveBeenCalledOnce());
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('step 2: back button returns to step 1', async () => {
    (commands.launcherImportDiscover as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: [mockForeign],
    });

    const { getByTestId, queryByTestId } = render(LauncherImportDialog, {
      props: { onClose: vi.fn() },
    });

    fireEvent.click(getByTestId('discover-btn'));
    await waitFor(() => expect(getByTestId('discovered-list')).toBeTruthy());
    fireEvent.click(
      getByTestId('discovered-list').querySelector('[data-testid="instance-row"]') as Element,
    );
    await waitFor(() => expect(getByTestId('back-btn')).toBeTruthy());

    fireEvent.click(getByTestId('back-btn'));
    await waitFor(() => expect(queryByTestId('import-btn')).toBeNull());
    expect(getByTestId('discover-btn')).toBeTruthy();
  });

  it('browse-to-folder calls launcherImportInspectFolder and advances to step 2', async () => {
    openFileMock.mockResolvedValue('/some/path');
    (commands.launcherImportInspectFolder as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: mockForeign,
    });

    const { getByTestId } = render(LauncherImportDialog, {
      props: { onClose: vi.fn() },
    });

    fireEvent.click(getByTestId('browse-folder-btn'));
    await waitFor(() => expect(getByTestId('import-btn')).toBeTruthy());
    expect(commands.launcherImportInspectFolder).toHaveBeenCalledWith('/some/path');
  });

  it('browse-to-folder: cancelled picker does nothing', async () => {
    openFileMock.mockResolvedValue(null);

    const { getByTestId, queryByTestId } = render(LauncherImportDialog, {
      props: { onClose: vi.fn() },
    });

    fireEvent.click(getByTestId('browse-folder-btn'));
    await waitFor(() => expect(queryByTestId('import-btn')).toBeNull());
    expect(getByTestId('discover-btn')).toBeTruthy();
  });
});

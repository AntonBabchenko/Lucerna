// Settings → Storage: open the data folder, and the free space on its drive.
// Both answer only for the real data folder — never a recovery session's
// throwaway root, never mid-move, never a figure for some other drive.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({
  getDataLocation: vi.fn(),
  dataRootSizeBytes: vi.fn(),
  dataRootFreeBytes: vi.fn(),
  openDataFolder: vi.fn(),
  modsCacheSizeBytes: vi.fn(),
  modsClearCache: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    getDataLocation: () => h.getDataLocation(),
    dataRootSizeBytes: () => h.dataRootSizeBytes(),
    dataRootFreeBytes: () => h.dataRootFreeBytes(),
    openDataFolder: () => h.openDataFolder(),
    modsCacheSizeBytes: () => h.modsCacheSizeBytes(),
    modsClearCache: () => h.modsClearCache(),
    appSettingsGet: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { general: { log_retention: { enabled: false, max_files: 10, max_total_mb: 100 } } },
    }),
    appSettingsPatchGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    restartBlocked: vi.fn().mockResolvedValue('none'),
  },
  events: { dataMigrationProgress: { listen: vi.fn().mockResolvedValue(() => {}) } },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushSuccess: vi.fn() }));

import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';
import { dataLocation } from '$lib/settings/data-location.svelte';
import StoragePanel from '$lib/settings/StoragePanel.svelte';

const status = (over: Record<string, unknown> = {}) => ({
  status: 'ok',
  data: {
    effective: '/data',
    configured: null,
    fell_back: false,
    default_dir: '/default',
    relocation: { kind: 'idle' },
    ...over,
  },
});
const TB = 2748779069440;

async function use(s: ReturnType<typeof status>) {
  h.getDataLocation.mockResolvedValue(s);
  await dataLocation.refresh();
}
const openBtn = () => screen.getByRole('button', { name: 'Open data folder' }) as HTMLButtonElement;
const freeRow = () => screen.getByText('Free on this drive:').closest('div') as HTMLElement;

beforeEach(async () => {
  vi.clearAllMocks();
  h.dataRootSizeBytes.mockResolvedValue({ status: 'ok', data: 4096 });
  h.dataRootFreeBytes.mockResolvedValue({ status: 'ok', data: TB });
  h.openDataFolder.mockResolvedValue({ status: 'ok', data: null });
  h.modsCacheSizeBytes.mockResolvedValue({ status: 'ok', data: 2048 });
  h.modsClearCache.mockResolvedValue({ status: 'ok', data: 2048 });
  __resetAppSettingsForTest();
  await loadAppSettings();
  await use(status());
});

describe('Open data folder', () => {
  it('opens it', async () => {
    render(StoragePanel);
    await fireEvent.click(openBtn());
    await waitFor(() => expect(h.openDataFolder).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(/Couldn't open the data folder/)).toBeNull();
  });

  it('says why when the folder is not there', async () => {
    h.openDataFolder.mockResolvedValue({
      status: 'error',
      error: { kind: 'data_root_unreachable', path: '/data', problem: { kind: 'missing' } },
    });
    render(StoragePanel);
    await fireEvent.click(openBtn());
    expect(
      await screen.findByText(/Couldn't open the data folder: Your data folder isn't there/),
    ).toBeTruthy();
  });

  it('says so even when the call itself failed', async () => {
    h.openDataFolder.mockRejectedValue(new Error('ipc gone'));
    render(StoragePanel);
    await fireEvent.click(openBtn());
    expect(await screen.findByText(/Couldn't open the data folder: ipc gone/)).toBeTruthy();
  });

  it('is there but disabled in a recovery session, and says why', async () => {
    await use(
      status({ fell_back: true, configured: 'D:\\Data', fallback: { kind: 'root_missing' } }),
    );
    render(StoragePanel);
    expect(openBtn().disabled).toBe(true);
    expect(screen.getByText(/Not available in a recovery session/)).toBeTruthy();
  });

  it('is disabled while the data folder is being moved, and says why', async () => {
    await use(status({ relocation: { kind: 'running', phase: 'copying' } }));
    render(StoragePanel);
    expect(openBtn().disabled).toBe(true);
    expect(screen.getByText(/while the data folder is being moved/)).toBeTruthy();
  });
});

describe('Free on this drive', () => {
  it('shows the free space of the data folder’s drive', async () => {
    render(StoragePanel);
    await waitFor(() => expect(freeRow().textContent).toContain('2.50 TB'));
  });

  it('reads 0 B on a full drive, never an empty value', async () => {
    h.dataRootFreeBytes.mockResolvedValue({ status: 'ok', data: 0 });
    render(StoragePanel);
    await waitFor(() => expect(freeRow().textContent).toContain('0 B'));
  });

  it("says couldn't be measured when it could not be, with the reason", async () => {
    h.dataRootFreeBytes.mockResolvedValue({
      status: 'error',
      error: { kind: 'data_root_unreachable', path: '/data', problem: { kind: 'not_a_data_root' } },
    });
    render(StoragePanel);
    await waitFor(() => expect(freeRow().textContent).toContain("couldn't be measured"));
    expect(screen.getByText(/no longer holds your data/)).toBeTruthy();
  });

  it('is never measured in a recovery session', async () => {
    await use(
      status({ fell_back: true, configured: 'D:\\Data', fallback: { kind: 'root_missing' } }),
    );
    render(StoragePanel);
    await new Promise((r) => setTimeout(r, 0));
    expect(h.dataRootFreeBytes).not.toHaveBeenCalled();
    expect(screen.queryByText('Free on this drive:')).toBeNull();
  });

  it('is measured again, with the size, after the cache is cleared', async () => {
    render(StoragePanel);
    await waitFor(() => expect(h.dataRootFreeBytes).toHaveBeenCalledTimes(1));
    const sizeCalls = h.dataRootSizeBytes.mock.calls.length;
    await fireEvent.click(await screen.findByRole('button', { name: 'Clear cache' }));
    await waitFor(() => expect(h.dataRootFreeBytes).toHaveBeenCalledTimes(2));
    expect(h.dataRootSizeBytes.mock.calls.length).toBeGreaterThan(sizeCalls);
  });
});

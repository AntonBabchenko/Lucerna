import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// The two IPC commands StoragePanel drives are wrapped in typedError on
// the real bindings, so they resolve to a `{ status: 'ok' | 'error' }`
// envelope rather than throwing. The plan's draft test in the v0.5.0
// mod browser plan used the raw-value shape (pre-typedError); adapted
// here to the result-status pattern Task 19 established.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsCacheSizeBytes: vi.fn(),
    modsClearCache: vi.fn(),
    appSettingsGet: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        general: {
          hide_to_tray_during_game: false,
          theme: 'system',
          check_updates_on_startup: true,
          gpu_preference: 'auto',
          log_retention: { enabled: false, max_files: 10, max_total_mb: 100 },
        },
      },
    }),
    appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    // StoragePanel calls dataLocation.init() -> getDataLocation() on mount.
    getDataLocation: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { effective: '/data', configured: null, fell_back: false },
    }),
    // Data-root size is loaded lazily via its own command (split from
    // getDataLocation so startup never walks the whole tree).
    dataRootSizeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 4096 }),
    setDataLocation: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    // The picker flow asks the backend to classify the picked folder before
    // any dialog opens; adopt commits through its own command.
    planDataLocationChange: vi.fn(),
    adoptDataLocation: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  // confirmPending subscribes to migration progress for migrate/reset.
  events: {
    dataMigrationProgress: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));

// The OS directory picker (plugin:dialog). Resolved value set per test.
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

const { pushSuccess } = vi.hoisted(() => ({ pushSuccess: vi.fn() }));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushSuccess }));

import StoragePanel from '$lib/settings/StoragePanel.svelte';

describe('StoragePanel', () => {
  it('shows size, clears cache, pushes a success toast, and disables Clear once empty', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsCacheSizeBytes as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce({ status: 'ok', data: 2048 })
      .mockResolvedValueOnce({ status: 'ok', data: 0 });
    (mod.commands.modsClearCache as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: 2048,
    });

    render(StoragePanel);
    // Yield once so the mount-time refresh() promise resolves before we
    // assert on the rendered size.
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/2\.0 KB/)).toBeTruthy();

    const button = screen.getByRole('button', { name: 'Clear cache' }) as HTMLButtonElement;
    expect(button.disabled).toBe(false);

    await fireEvent.click(button);
    // Clear chains modsClearCache → refresh(). Yield twice so both
    // microtasks settle before asserting on the toast + post-clear state.
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));

    expect(mod.commands.modsClearCache).toHaveBeenCalled();
    // Success is announced through the global toast store (auto-dismissing),
    // not an inline box that lingered forever.
    expect(pushSuccess).toHaveBeenCalledWith(expect.stringMatching(/Cache cleared/));
    // Second refresh reported zero bytes — the button must now be disabled.
    expect(
      (screen.getByRole('button', { name: 'Clear cache' }) as HTMLButtonElement).disabled,
    ).toBe(true);
  });

  it('renders the typed error when modsCacheSizeBytes fails', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsCacheSizeBytes as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'mods_cache_io', details: 'permission denied' },
    });

    render(StoragePanel);
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/permission denied/)).toBeTruthy();
  });

  it('loads the data-root size lazily via dataRootSizeBytes and renders it', async () => {
    const mod = await import('$lib/ipc/bindings');
    render(StoragePanel);
    // Yield so the mount-time refreshDataRootSize() promise resolves.
    await new Promise((r) => setTimeout(r, 0));
    expect(mod.commands.dataRootSizeBytes).toHaveBeenCalled();
    // 4096 bytes → "4.0 KB" (formatSize, 1024 divisor).
    expect(screen.getByText(/4\.0 KB/)).toBeTruthy();
  });

  it('shows a spinner in the size row while dataRootSizeBytes is pending', async () => {
    const mod = await import('$lib/ipc/bindings');
    // Hold the command unresolved so the row stays in its loading state.
    let resolveSize: (v: { status: 'ok'; data: number }) => void = () => {};
    (mod.commands.dataRootSizeBytes as ReturnType<typeof vi.fn>).mockReturnValue(
      new Promise((r) => {
        resolveSize = r;
      }),
    );

    render(StoragePanel);
    await new Promise((r) => setTimeout(r, 0));
    // Loading → the size row shows a status spinner, not "0 B".
    const sizeLabel = screen.getByText('Size on disk:');
    const row = sizeLabel.closest('div') as HTMLElement;
    expect(row.querySelector('[role="status"]')).not.toBeNull();

    // Resolve → the spinner is replaced by the formatted size.
    resolveSize({ status: 'ok', data: 4096 });
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/4\.0 KB/)).toBeTruthy();
  });
});

describe('StoragePanel — log retention', () => {
  it('renders the retention toggle and the two numeric inputs', () => {
    const { container } = render(StoragePanel);
    expect(container.querySelector('[data-testid="log-retention-toggle"]')).not.toBeNull();
    expect(container.querySelector('[data-testid="log-retention-max-files"]')).not.toBeNull();
    expect(container.querySelector('[data-testid="log-retention-max-mb"]')).not.toBeNull();
  });
});

describe('StoragePanel — data location change planning', () => {
  // Call lists must start clean: the adopt test asserts setDataLocation was
  // NOT called (and vice versa). clearAllMocks keeps the factory-installed
  // resolved values, it only clears recorded calls.
  beforeEach(() => {
    vi.clearAllMocks();
  });

  async function pickAndPlan(picked: string, plan: unknown) {
    const dialogPlugin = await import('@tauri-apps/plugin-dialog');
    (dialogPlugin.open as ReturnType<typeof vi.fn>).mockResolvedValue(picked);
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.planDataLocationChange as ReturnType<typeof vi.fn>).mockResolvedValue(plan);
    render(StoragePanel);
    await new Promise((r) => setTimeout(r, 0));
    await fireEvent.click(screen.getByRole('button', { name: 'Change location…' }));
    // Two yields: the picker promise, then the plan promise.
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));
    return mod;
  }

  it('offers to adopt when the backend classifies the pick as an existing root', async () => {
    const mod = await pickAndPlan('C:\\Programs\\Lucerna', {
      status: 'ok',
      data: { kind: 'adopt', path: 'C:\\Programs\\Lucerna\\LucernaData' },
    });
    expect(mod.commands.planDataLocationChange).toHaveBeenCalledWith('C:\\Programs\\Lucerna');
    // Adopt dialog, not the move dialog.
    expect(screen.getByText('Use existing data folder?')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Switch and restart' }));
    await new Promise((r) => setTimeout(r, 0));
    // Commits through the adopt command with the backend-planned path — the
    // migrate command (which would nest a fresh root) must never fire.
    expect(mod.commands.adoptDataLocation).toHaveBeenCalledWith(
      'C:\\Programs\\Lucerna\\LucernaData',
    );
    expect(mod.commands.setDataLocation).not.toHaveBeenCalled();
  });

  it('keeps the classic move flow when the backend plans a migration', async () => {
    const mod = await pickAndPlan('D:\\Games', {
      status: 'ok',
      data: { kind: 'migrate', path: 'D:\\Games\\LucernaData' },
    });
    expect(screen.getByText('Move data folder?')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Move and restart' }));
    await new Promise((r) => setTimeout(r, 0));
    expect(mod.commands.setDataLocation).toHaveBeenCalledWith('D:\\Games\\LucernaData');
    expect(mod.commands.adoptDataLocation).not.toHaveBeenCalled();
  });

  it('blocks re-picks while a plan classification is still in flight', async () => {
    const dialogPlugin = await import('@tauri-apps/plugin-dialog');
    (dialogPlugin.open as ReturnType<typeof vi.fn>).mockResolvedValue('C:\\Slow\\Drive');
    const mod = await import('$lib/ipc/bindings');
    // Hold the classification unresolved to model a stalling fs probe.
    let resolvePlan: (v: unknown) => void = () => {};
    (mod.commands.planDataLocationChange as ReturnType<typeof vi.fn>).mockReturnValue(
      new Promise((r) => {
        resolvePlan = r;
      }),
    );

    render(StoragePanel);
    await new Promise((r) => setTimeout(r, 0));
    const changeBtn = () =>
      screen.getByRole('button', { name: 'Change location…' }) as HTMLButtonElement;
    await fireEvent.click(changeBtn());
    await new Promise((r) => setTimeout(r, 0));
    // The button is disabled for the whole planning window, and a re-click
    // (e.g. via keyboard) never starts a second racing pick.
    expect(changeBtn().disabled).toBe(true);
    await fireEvent.click(changeBtn());
    await new Promise((r) => setTimeout(r, 0));
    expect(mod.commands.planDataLocationChange).toHaveBeenCalledTimes(1);

    resolvePlan({ status: 'ok', data: { kind: 'migrate', path: 'C:\\Slow\\Drive\\LucernaData' } });
    await new Promise((r) => setTimeout(r, 0));
    expect(changeBtn().disabled).toBe(false);
    expect(screen.getByText('Move data folder?')).toBeTruthy();
  });

  it('shows an inline message and no dialog when the pick is already current', async () => {
    const mod = await pickAndPlan('/data', {
      status: 'ok',
      data: { kind: 'already_current', path: '/data' },
    });
    expect(screen.getByText('That folder is already your current data folder.')).toBeTruthy();
    expect(screen.queryByText('Use existing data folder?')).toBeNull();
    expect(screen.queryByText('Move data folder?')).toBeNull();
    expect(mod.commands.adoptDataLocation).not.toHaveBeenCalled();
    expect(mod.commands.setDataLocation).not.toHaveBeenCalled();
  });

  it('keeps Reset enabled while fell_back and confirms a pointer-only reset', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.getDataLocation as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: {
        effective: 'C:\\Users\\u\\AppData\\Roaming\\com.lucerna.app',
        configured: 'C:\\Games\\LucernaData',
        fell_back: true,
      },
    });
    // The dataLocation store is a module singleton whose init() no-ops after
    // the first load — earlier tests already loaded configured:null. Force a
    // refresh so this test's status is actually applied.
    const { dataLocation } = await import('$lib/settings/data-location.svelte');
    await dataLocation.refresh();

    render(StoragePanel);
    await new Promise((r) => setTimeout(r, 0));

    // Reset is the ONLY in-app recovery from a configured folder that will
    // never come back — it must stay clickable in fallback, while Change
    // location (whose flows migrate) stays disabled.
    const resetBtn = screen.getByRole('button', {
      name: 'Reset to default',
    }) as HTMLButtonElement;
    expect(resetBtn.disabled).toBe(false);
    expect(
      (screen.getByRole('button', { name: 'Change location…' }) as HTMLButtonElement).disabled,
    ).toBe(true);

    await fireEvent.click(resetBtn);
    // Pointer-only copy: names the dead folder and promises no data move —
    // the normal reset body ("will copy … back") would be a lie here. The
    // path shows up twice by design: the fallback notice AND the dialog body.
    expect(screen.getByText(/detaches the unavailable folder/)).toBeTruthy();
    expect(screen.getAllByText(/Games\\LucernaData/).length).toBeGreaterThanOrEqual(2);

    await fireEvent.click(screen.getByRole('button', { name: 'Detach and restart' }));
    await new Promise((r) => setTimeout(r, 0));
    expect(mod.commands.setDataLocation).toHaveBeenCalledWith(null);
  });
});

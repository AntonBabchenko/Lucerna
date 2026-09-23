import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';

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
    // `patchGeneral` (the one settings write) goes through this; the refusal
    // test asserts it is NOT called.
    appSettingsPatchGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    // StoragePanel calls dataLocation.init() -> getDataLocation() on mount.
    getDataLocation: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        effective: '/data',
        configured: null,
        fell_back: false,
        default_dir: '/default',
        relocation: { kind: 'idle' },
      },
    }),
    // Data-root size is loaded lazily via its own command (split from getDataLocation so startup
    // never walks the whole tree).
    dataRootSizeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 4096 }),
    dataRootFreeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 1073741824 }),
    openDataFolder: vi.fn().mockResolvedValue({ status: 'ok', data: 'opened' }),
    // The move buttons are enabled ONLY on an exact 'none'.
    restartBlocked: vi.fn().mockResolvedValue('none'),
    // A clean move never returns — the backend restarts the app. A resolved `null` (the old
    // mock) would now throw on `data.kind`.
    setDataLocation: vi.fn().mockReturnValue(new Promise(() => {})),
    // Both flows ask the backend for a plan before any dialog opens; adopt commits through its
    // own command.
    planDataLocationChange: vi.fn(),
    planDataLocationReset: vi.fn(),
    adoptDataLocation: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  // Kept although the panel no longer subscribes: the dataLocation store imports `events`, and
  // only the app-level host ever calls attach().
  events: {
    dataMigrationProgress: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));

// The OS directory picker (plugin:dialog). Resolved value set per test.
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

const { pushSuccess } = vi.hoisted(() => ({ pushSuccess: vi.fn() }));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushSuccess }));

// A recovery session as the backend reports it: the session root is a throwaway dir.
const RECOVERY = {
  status: 'ok',
  data: {
    effective: 'C:\\Users\\u\\AppData\\Local\\com.lucerna.app\\recovery\\4242',
    configured: 'D:\\LucernaData',
    fell_back: true,
    fallback: { kind: 'root_not_writable', details: 'Access is denied.' },
    default_dir: 'C:\\Users\\u\\AppData\\Roaming\\com.lucerna.app',
    relocation: { kind: 'idle' },
  },
};

import { open as dialogOpen } from '@tauri-apps/plugin-dialog';
import { commands } from '$lib/ipc/bindings';
import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';
import { dataLocation } from '$lib/settings/data-location.svelte';
import StoragePanel from '$lib/settings/StoragePanel.svelte';
import { describedText } from './test-utils/aria';

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
    const sizeLabel = screen.getByText('Total size:');
    const row = sizeLabel.closest('div') as HTMLElement;
    expect(row.querySelector('[role="status"]')).not.toBeNull();

    // Resolve → the spinner is replaced by the formatted size.
    resolveSize({ status: 'ok', data: 4096 });
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/4\.0 KB/)).toBeTruthy();
  });

  it('formats the cache size against the app locale, not a hardcoded English unit', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsCacheSizeBytes as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: 2048,
    });
    // The panel defined a private fmt() with hardcoded B/KB/MB/GB three lines
    // below its own import of the localized formatSize, and used fmt() for the
    // cache row and the cleared toast while using formatSize for the data-root
    // row — two different renderings of a byte count in one panel.
    locale.set('ru');
    try {
      render(StoragePanel);
      await new Promise((r) => setTimeout(r, 0));
      expect(screen.getByText(/2,0 КБ/)).toBeTruthy();
    } finally {
      locale.set('en');
    }
  });
});

describe('StoragePanel — log retention', () => {
  it('renders the retention toggle and the two numeric inputs', () => {
    const { container } = render(StoragePanel);
    expect(container.querySelector('[data-testid="log-retention-toggle"]')).not.toBeNull();
    expect(container.querySelector('[data-testid="log-retention-max-files"]')).not.toBeNull();
    expect(container.querySelector('[data-testid="log-retention-max-mb"]')).not.toBeNull();
  });

  it('the enable checkbox is named by its title and described by its sentence; the TTL field by its hint', () => {
    render(StoragePanel);
    const cb = screen.getByRole('checkbox', { name: 'Automatically delete old logs' });
    expect(describedText(cb)).toContain('latest.log');
    // STOR-13: the limits cover crash reports and console logs too, and server
    // logs stay capped even when this is off — none of that was on screen.
    expect(describedText(cb)).toContain('crash reports');
    expect(describedText(cb)).toContain('15 files');
    // The TTL hint is linked by NumberField (10a); pinned here at the consumer.
    expect(describedText(screen.getByTestId('mod-metadata-ttl-days'))).toContain('never expire');
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
      data: {
        kind: 'migrate',
        path: 'D:\\Games\\LucernaData',
        required_bytes: 4096,
        free_bytes: 1024 ** 4,
      },
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
    // Regex name: the button is a BusyButton now, and while the plan is in flight its Spinner
    // (role="status", "Loading…") joins the accessible name.
    const changeBtn = () =>
      screen.getByRole('button', { name: /Change location…/ }) as HTMLButtonElement;
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
        default_dir: 'C:\\Users\\u\\AppData\\Roaming\\com.lucerna.app',
        relocation: { kind: 'idle' },
      },
    });
    // Reset plans first: while fallen back the backend answers "pointer only".
    (mod.commands.planDataLocationReset as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      data: {
        path: 'C:\\Users\\u\\AppData\\Roaming\\com.lucerna.app',
        pointer_only: true,
        required_bytes: 0,
        free_bytes: null,
        blocking_entries: [],
      },
    });
    // The dataLocation store is a module singleton whose init() no-ops after
    // the first load — earlier tests already loaded configured:null. Force a
    // refresh so this test's status is actually applied.
    await dataLocation.refresh();

    render(StoragePanel);
    await new Promise((r) => setTimeout(r, 0));

    // Both ways back stay clickable in a recovery session: Reset (pointer-only) for a folder that
    // will never come back, and Change location for one that moved (a drive letter changed, the
    // pointer went bad) — the backend refuses a MOVE plan there and accepts only an adopt. This
    // reverses #309, which pinned Change location disabled.
    const resetBtn = screen.getByRole('button', {
      name: 'Reset to default',
    }) as HTMLButtonElement;
    expect(resetBtn.disabled).toBe(false);
    expect(
      (screen.getByRole('button', { name: 'Change location…' }) as HTMLButtonElement).disabled,
    ).toBe(false);

    await fireEvent.click(resetBtn);
    // The dialog opens once the reset plan has resolved.
    await new Promise((r) => setTimeout(r, 0));
    // Pointer-only copy: names the dead folder and promises no data move —
    // the normal reset body ("will copy … back") would be a lie here.
    expect(screen.getByText(/detaches the unavailable folder/)).toBeTruthy();
    expect(screen.getAllByText(/Games\\LucernaData/).length).toBeGreaterThanOrEqual(1);

    await fireEvent.click(screen.getByRole('button', { name: 'Detach and restart' }));
    await new Promise((r) => setTimeout(r, 0));
    expect(mod.commands.setDataLocation).toHaveBeenCalledWith(null);
  });
});

const mock = (f: unknown) => f as ReturnType<typeof vi.fn>;
const flush = () => new Promise((r) => setTimeout(r, 0));
const GIB = 1024 ** 3;
// The full generated `restart_required` shape (both verdicts are the backend's).
const FINAL = {
  kind: 'restart_required',
  old_root: 'C:\\Old',
  new_root: 'D:\\Games\\LucernaData',
  leftovers: ['logs'],
  old_root_intact: false,
  old_root_is_default: false,
  retry_possible: true,
};
const MOVE_PLAN = {
  kind: 'migrate',
  path: 'D:\\Games\\LucernaData',
  required_bytes: 4096,
  free_bytes: 10 * GIB,
};
const RESET_PLAN = {
  path: 'C:\\Default',
  pointer_only: false,
  required_bytes: 4096,
  free_bytes: 10 * GIB,
  blocking_entries: [] as string[],
};
const LINKS = {
  status: 'error',
  error: { kind: 'data_location_invalid', reason: 'contains_links' },
};
const status = (over: Record<string, unknown> = {}) => ({
  status: 'ok',
  data: {
    effective: 'C:\\Old',
    configured: 'C:\\Old',
    fell_back: false,
    default_dir: 'C:\\Default',
    relocation: { kind: 'idle' },
    ...over,
  },
});

// `dataLocation` is a module singleton and earlier tests leave persistent mockResolvedValues
// behind — start every case from one known state.
async function resetIpc() {
  vi.clearAllMocks();
  mock(commands.modsCacheSizeBytes).mockResolvedValue({ status: 'ok', data: 0 });
  mock(commands.getDataLocation).mockResolvedValue(status());
  mock(commands.dataRootSizeBytes).mockResolvedValue({ status: 'ok', data: 4096 });
  mock(commands.restartBlocked).mockResolvedValue('none');
  mock(commands.setDataLocation).mockReturnValue(new Promise(() => {}));
  mock(commands.planDataLocationChange).mockResolvedValue({ status: 'ok', data: MOVE_PLAN });
  mock(commands.planDataLocationReset).mockResolvedValue({ status: 'ok', data: RESET_PLAN });
  mock(dialogOpen).mockResolvedValue('D:\\Games');
  await dataLocation.refresh();
}

// Regex names: both are BusyButtons, and a busy one holds its Spinner (role="status",
// "Loading…"), which joins the accessible name while the plan is in flight.
const change = () => screen.getByRole('button', { name: /Change location…/ }) as HTMLButtonElement;
const reset = () => screen.getByRole('button', { name: /Reset to default/ }) as HTMLButtonElement;
const settled = async () => {
  await flush();
  await flush();
};

async function mountPanel() {
  render(StoragePanel);
  await settled();
}

async function confirmMove() {
  await fireEvent.click(change());
  await settled();
  await fireEvent.click(screen.getByRole('button', { name: 'Move and restart' }));
  await settled();
}

describe('StoragePanel — the move is enabled only on an exact "none"', () => {
  beforeEach(resetIpc);

  it('enables both buttons and shows no reason', async () => {
    await mountPanel();
    expect(change().disabled || reset().disabled).toBe(false);
    expect(screen.queryByRole('button', { name: 'Check again' })).toBeNull();
  });

  it.each([
    ['running', /A game or server is running/],
    ['busy', /still busy with another operation/],
    ['unknown', /couldn't check whether a game or server is running/],
    ['a_token_from_the_future', /couldn't check whether/],
    [true, /couldn't check whether/], // the pre-change boolean
    [null, /couldn't check whether/],
  ])('blocks both buttons with one reason when the answer is %s', async (answer, reason) => {
    mock(commands.restartBlocked).mockResolvedValue(answer);
    await mountPanel();
    expect(change().disabled && reset().disabled).toBe(true);
    expect(screen.getByText(reason)).toBeTruthy();
  });

  it('reads a rejection as "could not tell" — the binding is not wrapped in typedError', async () => {
    mock(commands.restartBlocked).mockRejectedValue(new Error('ipc down'));
    await mountPanel();
    expect(change().disabled).toBe(true);
    expect(screen.getByText(/couldn't check whether/)).toBeTruthy();
  });

  it('stays blocked, and says why, while the answer is pending', async () => {
    mock(commands.restartBlocked).mockReturnValue(new Promise(() => {}));
    await mountPanel();
    expect(change().disabled).toBe(true);
    expect(screen.getByText(/Checking whether the data folder can be moved/)).toBeTruthy();
  });

  it('asks again on demand, so stopping the game needs no trip out of Settings', async () => {
    mock(commands.restartBlocked).mockResolvedValueOnce('running').mockResolvedValue('none');
    await mountPanel();
    await fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
    await flush();
    expect(change().disabled).toBe(false);
  });
});

describe('StoragePanel — every way a started move can come back', () => {
  beforeEach(resetIpc);

  it('hands a clean move to the app-level dialog and resets nothing while it runs', async () => {
    let settle: (v: unknown) => void = () => {};
    mock(commands.setDataLocation).mockReturnValue(
      new Promise((r) => {
        settle = r;
      }),
    );
    await mountPanel();
    await confirmMove();
    expect(commands.setDataLocation).toHaveBeenCalledWith('D:\\Games\\LucernaData');
    expect(screen.queryByText('Move data folder?')).toBeNull();
    expect(dataLocation.relocation.kind).toBe('running');
    expect(screen.queryByText(/Move cancelled/)).toBeNull();
    // Release the singleton for the tests that follow.
    settle({ status: 'ok', data: { kind: 'cancelled' } });
    await settled();
    expect(dataLocation.relocation.kind).toBe('idle');
  });

  it('says a cancelled move changed nothing, re-queries the gate, and takes focus back', async () => {
    mock(commands.setDataLocation).mockResolvedValue({ status: 'ok', data: { kind: 'cancelled' } });
    await mountPanel();
    await confirmMove();
    expect(screen.getByText('Move cancelled. Nothing was changed.')).toBeTruthy();
    expect(commands.restartBlocked).toHaveBeenCalledTimes(2);
    expect(document.activeElement).toBe(screen.getByTestId('data-location-section'));
  });

  it('shows a failure with the leftover copy, re-measures and re-queries', async () => {
    mock(commands.setDataLocation).mockResolvedValue({
      status: 'error',
      error: {
        kind: 'data_location_migration_failed',
        reason: 'disk full',
        partial_copy_left: 'D:\\Games\\LucernaData',
        restore_incomplete: false,
      },
    });
    await mountPanel();
    await confirmMove();
    expect(screen.getByText(/Moving the data folder failed: disk full\./)).toBeTruthy();
    expect(screen.getByText(/copy of the data was left at "D:\\Games\\LucernaData"/)).toBeTruthy();
    expect(commands.restartBlocked).toHaveBeenCalledTimes(2);
    expect(commands.dataRootSizeBytes).toHaveBeenCalledTimes(2);
    expect(dataLocation.relocation.kind).toBe('idle');
  });

  it('a REJECTED command (transport failure) still releases the app-level dialog and says why', async () => {
    // typedError rethrows real Error instances. Without a catch the store keeps `owned` forever:
    // a bare "Preparing…" dialog with no Cancel and no Restart — the one state a user cannot leave.
    mock(commands.setDataLocation).mockRejectedValue(new Error('ipc channel closed'));
    await mountPanel();
    await confirmMove();
    expect(dataLocation.relocation.kind).toBe('idle');
    expect(screen.getByText(/ipc channel closed/)).toBeTruthy();
    expect(commands.restartBlocked).toHaveBeenCalledTimes(2);
  });

  it('leaves restart_required to the app-level dialog: no error, no notice, state kept', async () => {
    mock(commands.setDataLocation).mockResolvedValue({
      status: 'ok',
      data: { kind: 'restart_required' },
    });
    await mountPanel();
    // init() no-ops after the first load, so this only feeds the settle-time read — the one that
    // carries the details.
    mock(commands.getDataLocation).mockResolvedValue(status({ relocation: FINAL }));
    await confirmMove();
    expect(dataLocation.relocation.kind).toBe('restart_required');
    expect(screen.queryByText(/Move cancelled|failed/)).toBeNull();
  });
});

describe('StoragePanel — reset goes through its own plan', () => {
  beforeEach(resetIpc);

  it('shows a spinner on Reset and locks Change while the plan is in flight', async () => {
    mock(commands.planDataLocationReset).mockReturnValue(new Promise(() => {}));
    await mountPanel();
    await fireEvent.click(reset());
    await flush();
    expect(reset().getAttribute('aria-busy')).toBe('true');
    expect(change().disabled).toBe(true);
  });

  it('confirms a real reset against the default folder and commits with null', async () => {
    await mountPanel();
    await fireEvent.click(reset());
    await flush();
    expect(screen.getByText('Reset to the default location?')).toBeTruthy();
    expect(screen.getByTestId('data-move-to').textContent?.trim()).toBe('C:\\Default');
    await fireEvent.click(screen.getByRole('button', { name: 'Move and restart' }));
    await flush();
    expect(commands.setDataLocation).toHaveBeenCalledWith(null);
  });

  it('lists what blocks the reset instead of opening a dialog', async () => {
    mock(commands.planDataLocationReset).mockResolvedValue({
      status: 'ok',
      data: { ...RESET_PLAN, blocking_entries: ['instances', 'servers'] },
    });
    await mountPanel();
    await fireEvent.click(reset());
    await flush();
    const box = screen.getByTestId('data-reset-blockers');
    expect(box.textContent).toContain('"C:\\Default" still holds 2 items');
    expect([...box.querySelectorAll('li')].map((li) => li.textContent)).toEqual([
      'instances',
      'servers',
    ]);
    expect(box.textContent).toContain('Keep data-location.json there');
    expect(screen.queryByText('Reset to the default location?')).toBeNull();
  });

  it.each([
    ['Reset', () => mock(commands.planDataLocationReset), reset],
    ['Change', () => mock(commands.planDataLocationChange), change],
  ])('refuses a tree with links on %s — before any confirm dialog', async (_label, cmd, btn) => {
    cmd().mockResolvedValue(LINKS);
    await mountPanel();
    await fireEvent.click(btn());
    await settled();
    expect(screen.getByText(/contains symbolic links/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Move and restart' })).toBeNull();
    expect(commands.setDataLocation).not.toHaveBeenCalled();
  });
});

describe('StoragePanel — an unknown size is not "0 B"', () => {
  beforeEach(resetIpc);

  it('shows an error state in the size row when the size call fails', async () => {
    mock(commands.dataRootSizeBytes).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: '<data_root_size>', details: 'access denied' },
    });
    await mountPanel();
    const row = screen.getByText('Total size:').closest('div') as HTMLElement;
    expect(row.textContent).toContain("couldn't be measured");
    expect(row.textContent).not.toContain('0 B');
    expect(screen.getByText(/access denied/)).toBeTruthy();
  });
  it('in a recovery session: Change location is enabled, the session path is never shown, and the log folder is one click away', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.getDataLocation as ReturnType<typeof vi.fn>).mockResolvedValue(RECOVERY);
    await dataLocation.refresh();
    render(StoragePanel);
    await new Promise((r) => setTimeout(r, 0));

    expect(
      (screen.getByRole('button', { name: 'Change location…' }) as HTMLButtonElement).disabled,
    ).toBe(false);
    // The throwaway root is an implementation detail: not as the current folder, not as a size.
    expect(screen.queryByText(/recovery\\4242/)).toBeNull();
    expect(screen.getByText('Temporary session')).toBeTruthy();
    expect(screen.queryByText('Total size:')).toBeNull();
    // The OS error lives here, in the Storage notice — never in the banner.
    expect(screen.getByText(/Access is denied/)).toBeTruthy();
    // With nothing seeded there is no instance, so the Logs popover has nothing to list: the
    // launcher log — the one document that says WHY — needs its own way in.
    expect(screen.getByRole('button', { name: 'Open log folder' })).toBeTruthy();
  });

  it('in a recovery session a folder without Lucerna data is refused inline, as its own sentence', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.getDataLocation as ReturnType<typeof vi.fn>).mockResolvedValue(RECOVERY);
    await dataLocation.refresh();
    (dialogOpen as ReturnType<typeof vi.fn>).mockResolvedValue('E:\\Empty');
    (mod.commands.planDataLocationChange as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'error',
      error: { kind: 'data_location_invalid', reason: 'fallback_adopt_only' },
    });
    render(StoragePanel);
    await new Promise((r) => setTimeout(r, 0));
    await fireEvent.click(screen.getByRole('button', { name: 'Change location…' }));
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/already holds Lucerna data/)).toBeTruthy();
    expect(screen.queryByText(/That folder can't be used/)).toBeNull();
    expect(mod.commands.setDataLocation).not.toHaveBeenCalled();
  });

  it('offers Reset even when the pointer could not be read — no path, and no "" in the copy', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.getDataLocation as ReturnType<typeof vi.fn>).mockResolvedValue({
      ...RECOVERY,
      data: { ...RECOVERY.data, configured: null, fallback: { kind: 'pointer_corrupt' } },
    });
    await dataLocation.refresh();
    render(StoragePanel);
    await new Promise((r) => setTimeout(r, 0));
    // The button used to render only when a path was configured — with a corrupt pointer there
    // was NO button for the action the copy names.
    expect(screen.getByRole('button', { name: 'Reset to default' })).toBeTruthy();
    expect(screen.queryByText(/""/)).toBeNull();
  });
});

// Runs last on purpose: two cases leave the dataLocation / appSettings
// singletons in a deliberate state.
describe('StoragePanel — one vocabulary (batch 10a)', () => {
  beforeEach(resetIpc);

  it('leads with the data folder: the block headings are in page order', async () => {
    await mountPanel();
    const headings = screen.getAllByRole('heading', { level: 3 }).map((h) => h.textContent?.trim());
    expect(headings).toEqual([
      'Data location',
      'Mod download cache',
      'Mod info cache',
      'Log retention',
    ]);
  });

  it('a failed clear re-measures the cache, so the real remaining size sits beside the error', async () => {
    mock(commands.modsCacheSizeBytes).mockResolvedValue({ status: 'ok', data: 2048 });
    mock(commands.modsClearCache).mockResolvedValue({
      status: 'error',
      error: { kind: 'mods_cache_io', details: 'locked' },
    });
    await mountPanel();
    await fireEvent.click(screen.getByRole('button', { name: 'Clear cache' }));
    await settled();
    expect(screen.getByText(/locked/)).toBeTruthy();
    // Once on mount, once after the failed clear.
    expect(commands.modsCacheSizeBytes).toHaveBeenCalledTimes(2);
  });

  it('a re-measure that also fails keeps the clear reason and stops claiming a size', async () => {
    mock(commands.modsCacheSizeBytes)
      .mockResolvedValueOnce({ status: 'ok', data: 2048 })
      .mockResolvedValueOnce({
        status: 'error',
        error: { kind: 'mods_cache_io', details: 'drive disconnected' },
      });
    mock(commands.modsClearCache).mockResolvedValue({
      status: 'error',
      error: { kind: 'mods_cache_io', details: 'locked' },
    });
    await mountPanel();
    await fireEvent.click(screen.getByRole('button', { name: 'Clear cache' }));
    await settled();
    // The recovery step's own failure must not overwrite the reason the clear failed.
    expect(screen.getByText(/locked/)).toBeTruthy();
    expect(screen.getByText(/drive disconnected/)).toBeTruthy();
    // And the size measured BEFORE the clear is no longer presented as the size now.
    expect(screen.queryByText(/2\.0 KB/)).toBeNull();
    expect(screen.getByText("couldn't be measured")).toBeTruthy();
  });

  it('the size error is announced (role=alert)', async () => {
    mock(commands.dataRootSizeBytes).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: '<data_root_size>', details: 'access denied' },
    });
    await mountPanel();
    expect(screen.getByText(/access denied/).closest('[role="alert"]')).not.toBeNull();
  });

  it('"Keep newest logs" refuses 0: the saved value stays, the message names the minimum, nothing is patched', async () => {
    __resetAppSettingsForTest();
    mock(commands.appSettingsGet).mockResolvedValueOnce({
      status: 'ok',
      data: {
        general: {
          hide_to_tray_during_game: false,
          theme: 'system',
          check_updates_on_startup: true,
          gpu_preference: 'auto',
          log_retention: { enabled: true, max_files: 10, max_total_mb: 100 },
        },
      },
    });
    await loadAppSettings();
    await mountPanel();
    const input = screen.getByTestId('log-retention-max-files') as HTMLInputElement;
    await waitFor(() => expect(input.disabled).toBe(false));
    input.value = '0';
    await fireEvent.change(input);
    expect(input.value).toBe('10');
    expect(screen.getByText('Enter a whole number of at least 1.')).toBeTruthy();
    expect(commands.appSettingsPatchGeneral).not.toHaveBeenCalled();
  });

  it('a data-location read failure is announced (role=alert)', async () => {
    mock(commands.getDataLocation).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'data-location.json', details: 'pointer unreadable' },
    });
    await dataLocation.refresh();
    await mountPanel();
    expect(screen.getByText(/pointer unreadable/).closest('[role="alert"]')).not.toBeNull();
  });
});

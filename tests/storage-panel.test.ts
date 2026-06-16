import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

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
  },
}));

import StoragePanel from '$lib/settings/StoragePanel.svelte';

describe('StoragePanel', () => {
  it('shows size, clears cache, shows toast, and disables Clear once empty', async () => {
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
    expect(screen.getByText(/Cache cleared/)).toBeTruthy();
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
});

describe('StoragePanel — log retention', () => {
  it('renders the retention toggle and the two numeric inputs', () => {
    const { container } = render(StoragePanel);
    expect(container.querySelector('[data-testid="log-retention-toggle"]')).not.toBeNull();
    expect(container.querySelector('[data-testid="log-retention-max-files"]')).not.toBeNull();
    expect(container.querySelector('[data-testid="log-retention-max-mb"]')).not.toBeNull();
  });
});

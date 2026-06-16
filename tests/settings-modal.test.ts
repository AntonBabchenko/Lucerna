import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'missing' }),
    modsSetCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsClearCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsCacheSizeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsClearCache: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
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
    updateCheck: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { available: false, current: '0.0.0' } }),
    gpuCapability: vi.fn().mockResolvedValue({ status: 'ok', data: { kind: 'unsupported' } }),
  },
}));

import SettingsModal from '$lib/settings/SettingsModal.svelte';
import { settingsOpen } from '$lib/settings/state.svelte';

afterEach(() => {
  settingsOpen.value = null;
});

describe('SettingsModal', () => {
  it('renders 7 section tabs and closes on Escape', async () => {
    settingsOpen.value = { tab: 'appearance' };
    render(SettingsModal);
    expect(screen.getByRole('dialog', { name: 'Settings' })).toBeTruthy();
    expect(screen.getAllByRole('tab')).toHaveLength(7);
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(settingsOpen.value).toBe(null);
  });

  it('opens on the Appearance section by default and shows theme controls', () => {
    settingsOpen.value = { tab: 'appearance' };
    render(SettingsModal);
    expect(screen.getByRole('tab', { name: 'Appearance' }).getAttribute('aria-selected')).toBe(
      'true',
    );
    expect(screen.getByTestId('theme-system')).toBeTruthy();
  });

  it('deep-links to Integrations and mounts the CurseForge form', () => {
    settingsOpen.value = { tab: 'integrations' };
    render(SettingsModal);
    expect(screen.getByRole('tab', { name: 'Integrations' }).getAttribute('aria-selected')).toBe(
      'true',
    );
    expect(screen.getByText(/Status:/)).toBeTruthy();
  });

  it('switches to the About section on click and shows the disclaimer', async () => {
    settingsOpen.value = { tab: 'appearance' };
    render(SettingsModal);
    await fireEvent.click(screen.getByRole('tab', { name: 'About' }));
    expect(screen.getByText(/NOT AN OFFICIAL MINECRAFT PRODUCT\./)).toBeTruthy();
  });

  it('ArrowDown moves the active section to the next one', async () => {
    settingsOpen.value = { tab: 'appearance' };
    render(SettingsModal);
    const tablist = screen.getByRole('tablist');
    await fireEvent.keyDown(tablist, { key: 'ArrowDown' });
    expect(screen.getByRole('tab', { name: 'Game' }).getAttribute('aria-selected')).toBe('true');
  });

  it('shows the changelog under Updates, not under About', async () => {
    settingsOpen.value = { tab: 'updates' };
    render(SettingsModal);
    expect(screen.getByText("What's new")).toBeTruthy();
    expect(screen.getByText('v0.1.0')).toBeTruthy();
  });
});

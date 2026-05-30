import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

// SettingsModal mounts the real CurseForgeKeyForm (Task 19) on the
// CurseForge tab and the real StoragePanel (Task 20) on the Storage
// tab. Both fire IPC calls on mount, so stub every command they touch
// — otherwise `__TAURI_INVOKE` (undefined in happy-dom) produces an
// unhandled rejection in this shell-only test file.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'missing' }),
    modsSetCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsClearCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsCacheSizeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsClearCache: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    // GeneralPanel (new default tab) calls these on mount.
    appSettingsGet: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        general: { hide_to_tray_on_launch: false, theme: 'system', replay_tour_pending: false },
      },
    }),
    appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    appSettingsMarkTourCompleted: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
}));

import SettingsModal from '$lib/settings/SettingsModal.svelte';
import { settingsOpen } from '$lib/settings/state.svelte';

// `settingsOpen` is a module-level $state rune shared across all tests
// in this file (same module instance). Reset it between cases so state
// doesn't bleed from one test into the next.
afterEach(() => {
  settingsOpen.value = null;
});

describe('SettingsModal', () => {
  it('renders when settingsOpen is set and closes on Escape', async () => {
    settingsOpen.value = { tab: 'curseforge' };
    render(SettingsModal);
    expect(screen.getByRole('dialog', { name: 'Settings' })).toBeTruthy();
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(settingsOpen.value).toBe(null);
  });

  it('switches tabs on click', async () => {
    settingsOpen.value = { tab: 'curseforge' };
    render(SettingsModal);
    const storageTab = screen.getByRole('tab', { name: 'Storage' });
    await fireEvent.click(storageTab);
    expect(storageTab.getAttribute('aria-selected')).toBe('true');
    // And the CurseForge tab is no longer selected.
    expect(screen.getByRole('tab', { name: 'CurseForge' }).getAttribute('aria-selected')).toBe(
      'false',
    );
  });

  it('exposes an About tab that mounts the AboutPanel', async () => {
    settingsOpen.value = { tab: 'curseforge' };
    render(SettingsModal);
    const aboutTab = screen.getByRole('tab', { name: 'About' });
    await fireEvent.click(aboutTab);
    expect(aboutTab.getAttribute('aria-selected')).toBe('true');
    // Disclaimer comes from the panel, sourced from the shared constant.
    expect(
      screen.getByText(
        /NOT AN OFFICIAL MINECRAFT PRODUCT\. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT\./,
      ),
    ).toBeTruthy();
  });
});

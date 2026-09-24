import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'missing' }),
    modsSetCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsClearCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsCacheSizeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsClearCache: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    // AiTranslationSection (the third Integrations block) reads the stored-key
    // status for the configured provider on mount.
    l10nPrefillKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: false }),
    l10nPrefillSetKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    l10nPrefillTestKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    l10nPrefillProviderDefaults: vi.fn().mockResolvedValue([]),
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
    gpuCapability: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { mechanism: 'none', capability: { kind: 'unsupported' } },
    }),
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
    dataRootSizeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    dataRootFreeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 1073741824 }),
    openDataFolder: vi.fn().mockResolvedValue({ status: 'ok', data: 'opened' }),
    // The move buttons are enabled only on an exact 'none'.
    restartBlocked: vi.fn().mockResolvedValue('none'),
    // A clean move never returns (the backend restarts the app).
    setDataLocation: vi.fn().mockReturnValue(new Promise(() => {})),
  },
}));

import { commands } from '$lib/ipc/bindings';
import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';
import SettingsModal from '$lib/settings/SettingsModal.svelte';
import {
  closeSettings,
  openSettingsAt,
  settingsJumpFocus,
  settingsOpen,
  settingsSearchFocus,
} from '$lib/settings/state.svelte';

const frame = (): Promise<void> => new Promise((r) => requestAnimationFrame(() => r()));
const microtask = (): Promise<void> => new Promise((r) => queueMicrotask(() => r()));

afterEach(() => {
  settingsOpen.value = null;
  settingsSearchFocus.value = null;
  settingsJumpFocus.value = null;
});

describe('SettingsModal', () => {
  it('renders 8 section tabs and closes on Escape', async () => {
    settingsOpen.value = { tab: 'appearance' };
    render(SettingsModal);
    expect(screen.getByRole('dialog', { name: 'Settings' })).toBeTruthy();
    expect(screen.getAllByRole('tab')).toHaveLength(8);
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
    // Heading, not bare text — the panel renders the real CHANGELOG.md, so a
    // release note naming the "What's new" panel adds a second match.
    expect(screen.getByRole('heading', { name: "What's new" })).toBeTruthy();
    expect(screen.getByText('v0.1.0')).toBeTruthy();
  });

  it('opens with the search box focused, not the close button', async () => {
    settingsOpen.value = { tab: 'appearance' };
    render(SettingsModal);
    await frame();
    // The Appearance panel has its own comboboxes (theme, language): name the search one.
    const search = document.querySelector('input[data-autofocus]');
    expect(search).not.toBeNull();
    expect(search?.getAttribute('role')).toBe('combobox');
    expect(document.activeElement).toBe(search);
  });

  it('a pending jump that was never delivered is dropped when the user changes tab', async () => {
    settingsOpen.value = { tab: 'appearance' };
    render(SettingsModal);
    // Storage is not mounted, so nothing could have consumed this.
    settingsSearchFocus.value = 'storage.cache';
    await fireEvent.click(screen.getByRole('tab', { name: 'Game' }));
    expect(settingsSearchFocus.value).toBe(null);
  });

  it('a delivered jump is consumed, so returning to the tab does not flash it again', async () => {
    settingsOpen.value = { tab: 'appearance' };
    render(SettingsModal);
    await openSettingsAt('storage.cache');
    await microtask();
    expect(settingsSearchFocus.value).toBe(null);
    await fireEvent.click(screen.getByRole('tab', { name: 'Appearance' }));
    await fireEvent.click(screen.getByRole('tab', { name: 'Storage' }));
    const wrapper = document.querySelector('[data-search-anchor="storage.cache"]');
    expect(wrapper).not.toBeNull();
    expect(wrapper?.classList.contains('field-flash')).toBe(false);
  });

  // Batch 12e: a Change link on the Privacy page unmounts with its panel, so the
  // jump must put focus on the setting itself, not drop it to <body>.
  it('Change on the Privacy page lands on the setting, focuses it, and says where it went', async () => {
    __resetAppSettingsForTest();
    await loadAppSettings();
    settingsOpen.value = { tab: 'privacy' };
    render(SettingsModal);
    const row = screen.getByTestId('privacy-row-serverPing');
    await fireEvent.click(within(row).getByRole('button', { name: 'Change' }));
    await vi.waitFor(() =>
      expect(screen.getByRole('tab', { name: 'Game' }).getAttribute('aria-selected')).toBe('true'),
    );
    await vi.waitFor(() =>
      expect(document.activeElement).toBe(screen.getByTestId('server-ping-toggle')),
    );
    const wrapper = document.querySelector('[data-search-anchor="game.serverPing"]');
    expect(wrapper?.classList.contains('field-flash')).toBe(true);
    const live = document.querySelector('.sr-only[role="status"]');
    expect(live?.textContent).toContain('Opened setting: Saved server status, Game');
    // Delivered ⇒ consumed: the next flash of any field stays a plain flash.
    expect(settingsJumpFocus.value).toBe(null);
  });

  it('the About pointer opens the Privacy page with focus on its heading', async () => {
    settingsOpen.value = { tab: 'about' };
    render(SettingsModal);
    await fireEvent.click(screen.getByRole('button', { name: 'See the connections you control' }));
    await vi.waitFor(() =>
      expect(
        screen.getByRole('tab', { name: 'Privacy & network' }).getAttribute('aria-selected'),
      ).toBe('true'),
    );
    await vi.waitFor(() =>
      expect(document.activeElement).toBe(
        screen.getByRole('heading', { name: 'Connections you control' }),
      ),
    );
    // The page's label IS the tab's name: say it once.
    const live = document.querySelector('.sr-only[role="status"]');
    expect(live?.textContent).toBe('Opened setting: Privacy & network');
  });

  it('a jump while the settings are still being read keeps focus in the dialog, then hands it to the control', async () => {
    type Read = Awaited<ReturnType<typeof commands.appSettingsGet>>;
    let answer: (v: Read) => void = () => {};
    vi.mocked(commands.appSettingsGet).mockReturnValueOnce(
      new Promise<Read>((r) => {
        answer = r;
      }),
    );
    __resetAppSettingsForTest();
    void loadAppSettings();
    settingsOpen.value = { tab: 'privacy' };
    render(SettingsModal);
    const row = screen.getByTestId('privacy-row-serverPing');
    await fireEvent.click(within(row).getByRole('button', { name: 'Change' }));
    await vi.waitFor(() =>
      expect(screen.getByRole('tab', { name: 'Game' }).getAttribute('aria-selected')).toBe('true'),
    );
    const wrapper = document.querySelector('[data-search-anchor="game.serverPing"]');
    await vi.waitFor(() => expect(document.activeElement).toBe(wrapper));
    answer({ status: 'ok', data: { general: { allow_server_ping: false } } } as Read);
    await vi.waitFor(() =>
      expect(document.activeElement).toBe(screen.getByTestId('server-ping-toggle')),
    );
  });

  it('a tab picked before an in-modal jump is answered wins, and nothing takes focus', async () => {
    __resetAppSettingsForTest();
    await loadAppSettings();
    settingsOpen.value = { tab: 'privacy' };
    render(SettingsModal);
    const change = within(screen.getByTestId('privacy-row-serverPing')).getByRole('button', {
      name: 'Change',
    });
    // Both clicks land before Svelte flushes: the jump has not been answered yet.
    change.click();
    screen.getByRole('tab', { name: 'Appearance' }).click();
    await microtask();
    await frame();
    expect(screen.getByRole('tab', { name: 'Appearance' }).getAttribute('aria-selected')).toBe(
      'true',
    );
    expect(settingsJumpFocus.value).toBe(null);
    expect(settingsSearchFocus.value).toBe(null);
  });

  it('closing Settings drops an in-modal jump that was never delivered', () => {
    settingsJumpFocus.value = 'game.serverPing';
    closeSettings();
    expect(settingsJumpFocus.value).toBe(null);
  });
});

import { render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

// The same mock block as settings-modal.test.ts, with the store reporting a recovery session.
// Its own file: `dataLocation` is a module singleton and the notice must be seen with a status
// the other suite never feeds it.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'missing' }),
    modsSetCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsClearCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsCacheSizeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsClearCache: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    l10nPrefillKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: false }),
    l10nPrefillSetKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    l10nPrefillTestKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
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
    getDataLocation: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        effective: 'C:\\Users\\u\\AppData\\Local\\com.lucerna.app\\recovery\\4242',
        configured: 'D:\\LucernaData',
        fell_back: true,
        fallback: { kind: 'root_not_writable', details: 'Access is denied.' },
        default_dir: '/default',
        relocation: { kind: 'idle' },
      },
    }),
    dataRootSizeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    restartBlocked: vi.fn().mockResolvedValue('none'),
    setDataLocation: vi.fn().mockReturnValue(new Promise(() => {})),
  },
}));

import SettingsModal from '$lib/settings/SettingsModal.svelte';
import { dataLocation } from '$lib/settings/data-location.svelte';
import { settingsOpen } from '$lib/settings/state.svelte';

afterEach(() => {
  settingsOpen.value = null;
});

describe('SettingsModal in a recovery session', () => {
  it('says once, at the top, that settings changed here are not saved to the data folder', async () => {
    await dataLocation.refresh();
    settingsOpen.value = { tab: 'appearance' };
    render(SettingsModal);
    const notice = screen.getByTestId('settings-recovery-notice');
    expect(notice.textContent).toContain('not saved to your data folder');
    // Worded about the DATA FOLDER on purpose: keyring-backed keys DO persist, so "nothing you
    // change is kept" would be false.
    expect(notice.textContent).not.toMatch(/nothing you change/i);
  });
});

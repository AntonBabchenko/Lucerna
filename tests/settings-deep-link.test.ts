// A deep link into the CurseForge key field (the banners' path) must end with
// focus IN the field: the right target inside the wrapper, a field that is
// still disabled while the status loads, and a dialog whose own initial
// focus would otherwise take it back — all three in one chain.
import { render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsGetCurseforgeKeyStatus: vi.fn(async () => {
      await new Promise((r) => setTimeout(r, 0));
      return { status: 'ok', data: 'missing' };
    }),
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
    // The move buttons are enabled only on an exact 'none'.
    restartBlocked: vi.fn().mockResolvedValue('none'),
    // A clean move never returns (the backend restarts the app).
    setDataLocation: vi.fn().mockReturnValue(new Promise(() => {})),
  },
}));

import SettingsModal from '$lib/settings/SettingsModal.svelte';
import { openSettingsAt, settingsOpen, settingsSearchFocus } from '$lib/settings/state.svelte';

const frame = (): Promise<void> => new Promise((r) => requestAnimationFrame(() => r()));
const timer = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

afterEach(() => {
  settingsOpen.value = null;
  settingsSearchFocus.value = null;
  document.body.innerHTML = '';
});

describe('a deep link into the CurseForge key field', () => {
  it('ends with focus in the key input once it is usable', async () => {
    render(SettingsModal);
    await openSettingsAt('integrations.curseforgeKey');
    await frame();
    await timer();
    await timer();
    await Promise.resolve();
    expect(document.activeElement).toBe(screen.getByTestId('cf-key-input'));
  });
});

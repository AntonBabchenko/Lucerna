import { beforeEach, describe, expect, it, vi } from 'vitest';

const { windowSetCompact, appSettingsGet, appSettingsSetGeneral } = vi.hoisted(() => {
  const sampleGeneral = {
    hide_to_tray_during_game: false,
    theme: 'system',
    check_updates_on_startup: true,
    language: 'system',
    explanation_level: 'basic',
    compact_mode: false,
  };
  return {
    windowSetCompact: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    appSettingsGet: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { general: sampleGeneral },
    }),
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: { windowSetCompact, appSettingsGet, appSettingsSetGeneral },
}));

import { compactState, initCompact, setCompact, toggleCompact } from '$lib/layout/compact.svelte';

describe('compact mode rune module', () => {
  beforeEach(() => {
    compactState.value = false;
    windowSetCompact.mockClear();
    appSettingsGet.mockClear();
    appSettingsSetGeneral.mockClear();
  });

  it('setCompact flips the rune, resizes the window, and persists the flag', async () => {
    await setCompact(true);
    expect(compactState.value).toBe(true);
    // Height is null here: jsdom has no rendered sidebar to measure.
    expect(windowSetCompact).toHaveBeenCalledWith(true, null);
    expect(appSettingsSetGeneral).toHaveBeenCalledTimes(1);
    expect(appSettingsSetGeneral.mock.calls[0][0]).toMatchObject({ compact_mode: true });
  });

  it('toggleCompact inverts the current value', async () => {
    compactState.value = false;
    await toggleCompact();
    expect(compactState.value).toBe(true);
    expect(windowSetCompact).toHaveBeenLastCalledWith(true, null);
  });

  it('initCompact applies the persisted mode WITHOUT re-persisting', async () => {
    await initCompact(true);
    expect(compactState.value).toBe(true);
    expect(windowSetCompact).toHaveBeenCalledWith(true, null);
    expect(appSettingsSetGeneral).not.toHaveBeenCalled();
  });

  it('initCompact(false) applies expanded constraints without persisting', async () => {
    await initCompact(false);
    expect(compactState.value).toBe(false);
    // Still calls the backend (to apply the min-height floor) but does not persist.
    expect(windowSetCompact).toHaveBeenCalledWith(false, null);
    expect(appSettingsSetGeneral).not.toHaveBeenCalled();
  });

  it('setCompact rolls the rune back and does not persist when the resize fails', async () => {
    windowSetCompact.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'window_io', details: 'x' },
    });
    await setCompact(true);
    expect(compactState.value).toBe(false); // rolled back
    expect(appSettingsSetGeneral).not.toHaveBeenCalled();
  });
});

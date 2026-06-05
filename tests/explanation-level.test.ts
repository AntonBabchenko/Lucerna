import { beforeEach, describe, expect, it, vi } from 'vitest';

const appSettingsGet = vi.fn();
const appSettingsSetGeneral = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: () => appSettingsGet(),
    appSettingsSetGeneral: (g: unknown) => appSettingsSetGeneral(g),
  },
}));

import {
  explanationState,
  initExplanationLevel,
  setExplanationLevel,
} from '$lib/onboarding/explanation-level.svelte';

beforeEach(() => {
  appSettingsGet.mockReset();
  appSettingsSetGeneral.mockReset();
  explanationState.level = 'basic';
});

describe('initExplanationLevel', () => {
  it('adopts the persisted level', async () => {
    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { general: { explanation_level: 'advanced' } },
    });
    await initExplanationLevel();
    expect(explanationState.level).toBe('advanced');
  });

  it('falls back to basic when general has no level', async () => {
    appSettingsGet.mockResolvedValue({ status: 'ok', data: { general: {} } });
    await initExplanationLevel();
    expect(explanationState.level).toBe('basic');
  });

  it('keeps the default and does not throw when the read fails', async () => {
    appSettingsGet.mockResolvedValue({ status: 'error', error: 'x' });
    await initExplanationLevel();
    expect(explanationState.level).toBe('basic');
  });
});

describe('setExplanationLevel', () => {
  it('updates the rune and persists a merged general object', async () => {
    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { general: { theme: 'dark', explanation_level: 'basic' } },
    });
    appSettingsSetGeneral.mockResolvedValue({ status: 'ok', data: null });
    await setExplanationLevel('advanced');
    expect(explanationState.level).toBe('advanced');
    expect(appSettingsSetGeneral).toHaveBeenCalledWith({
      theme: 'dark',
      explanation_level: 'advanced',
    });
  });
});

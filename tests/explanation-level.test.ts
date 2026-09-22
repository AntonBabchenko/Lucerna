import { beforeEach, describe, expect, it, vi } from 'vitest';

// The tips level persists through the one settings contract: a field-level
// patch, a rollback when refused, and — the guard its siblings already had —
// never a rollback of a newer pick.
const appSettingsGet = vi.fn();
const appSettingsSetGeneral = vi.fn();
const appSettingsPatchGeneral = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: () => appSettingsGet(),
    appSettingsSetGeneral: (g: unknown) => appSettingsSetGeneral(g),
    appSettingsPatchGeneral: (p: unknown) => appSettingsPatchGeneral(p),
  },
}));

import { explanationState, setExplanationLevel } from '$lib/onboarding/explanation-level.svelte';
import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';

const GENERAL = { theme: 'dark', explanation_level: 'basic' };
const REFUSED = { status: 'error', error: { kind: 'io', path: 'app.json', details: 'x' } };

beforeEach(async () => {
  appSettingsGet.mockReset().mockResolvedValue({ status: 'ok', data: { general: GENERAL } });
  appSettingsSetGeneral.mockReset().mockResolvedValue({ status: 'ok', data: null });
  appSettingsPatchGeneral
    .mockReset()
    .mockImplementation(async (p: object) => ({ status: 'ok', data: { ...GENERAL, ...p } }));
  __resetAppSettingsForTest();
  await loadAppSettings();
  explanationState.level = 'basic';
});

describe('setExplanationLevel', () => {
  it('updates the rune and patches only the level', async () => {
    await setExplanationLevel('advanced');
    expect(explanationState.level).toBe('advanced');
    expect(appSettingsPatchGeneral).toHaveBeenCalledWith({ explanation_level: 'advanced' });
  });

  it('rolls the rune back when the patch is refused', async () => {
    appSettingsPatchGeneral.mockResolvedValueOnce(REFUSED);
    await setExplanationLevel('advanced');
    expect(explanationState.level).toBe('basic');
  });

  it('a stale failure does not revert a newer pick', async () => {
    let failFirst!: (v: unknown) => void;
    appSettingsPatchGeneral.mockReturnValueOnce(new Promise((r) => (failFirst = r)));
    const first = setExplanationLevel('advanced');
    const second = setExplanationLevel('basic'); // on screen at once, queued behind the first
    expect(explanationState.level).toBe('basic');
    failFirst(REFUSED);
    await first;
    expect(explanationState.level).toBe('basic'); // the refusal did not revert the newer pick
    await second;
    expect(explanationState.level).toBe('basic');
  });
});

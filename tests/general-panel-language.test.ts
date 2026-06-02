import { fireEvent, render, screen } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { beforeEach, describe, expect, test, vi } from 'vitest';

const appSettingsGet = vi.fn().mockResolvedValue({
  status: 'ok',
  data: {
    version: 1,
    active_instance: null,
    onboarding: { tour_completed_version: null },
    general: { hide_to_tray_during_game: false, theme: 'system', language: 'system' },
  },
});
const appSettingsSetGeneral = vi.fn().mockResolvedValue({ status: 'ok', data: null });

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: (...a: unknown[]) => appSettingsGet(...a),
    appSettingsSetGeneral: (...a: unknown[]) => appSettingsSetGeneral(...a),
    appSettingsMarkTourCompleted: vi.fn(),
    updateCheck: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { available: false, current: '0.0.0' } }),
  },
}));

import { locale } from '../src/lib/i18n';
import GeneralPanel from '../src/lib/settings/GeneralPanel.svelte';
import { settingsOpen } from '../src/lib/settings/state.svelte';

beforeEach(() => {
  settingsOpen.value = { tab: 'general' };
  locale.set('en');
  appSettingsSetGeneral.mockClear();
});

describe('GeneralPanel language picker', () => {
  test('renders a language selector', () => {
    render(GeneralPanel);
    expect(screen.getByTestId('language-select')).toBeTruthy();
  });

  test('choosing Russian switches the live locale and persists language', async () => {
    render(GeneralPanel);
    await fireEvent.change(screen.getByTestId('language-select'), { target: { value: 'ru' } });
    expect(get(locale)).toBe('ru');
    expect(appSettingsSetGeneral).toHaveBeenCalled();
    expect(appSettingsSetGeneral.mock.calls.at(-1)?.[0]).toMatchObject({ language: 'ru' });
  });

  test('M1: toggling a checkbox after language is changed does not clobber the persisted language', async () => {
    // Simulate: onMount reads language:'system', then the language picker persists
    // language:'ru' via its own RMW — subsequent reads return 'ru'. save() must
    // do a fresh read-modify-write and preserve 'ru', not revert to 'system'.
    appSettingsGet.mockReset();
    appSettingsGet
      .mockResolvedValueOnce({
        status: 'ok',
        data: {
          version: 1,
          active_instance: null,
          onboarding: { tour_completed_version: null },
          general: {
            hide_to_tray_during_game: false,
            theme: 'system',
            language: 'system',
            check_updates_on_startup: true,
          },
        },
      })
      .mockResolvedValue({
        status: 'ok',
        data: {
          version: 1,
          active_instance: null,
          onboarding: { tour_completed_version: null },
          general: {
            hide_to_tray_during_game: false,
            theme: 'system',
            language: 'ru',
            check_updates_on_startup: true,
          },
        },
      });

    render(GeneralPanel);
    // Wait for onMount to complete (first appSettingsGet call).
    await new Promise((r) => setTimeout(r, 0));

    // Fire the tray checkbox change to trigger save().
    await fireEvent.change(screen.getByTestId('tray-toggle'));

    // save() must not clobber the 'ru' that the language picker already persisted.
    expect(appSettingsSetGeneral).toHaveBeenCalled();
    expect(appSettingsSetGeneral.mock.calls.at(-1)?.[0]).toMatchObject({ language: 'ru' });
  });
});

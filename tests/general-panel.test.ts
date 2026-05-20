import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, test, vi } from 'vitest';

const appSettingsMarkTourCompleted = vi.fn();
const appSettingsGet = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: (...args: unknown[]) => appSettingsGet(...args),
    appSettingsMarkTourCompleted: (...args: unknown[]) => appSettingsMarkTourCompleted(...args),
  },
}));

import { tourState } from '../src/lib/onboarding/state.svelte';
import GeneralPanel from '../src/lib/settings/GeneralPanel.svelte';
import { settingsOpen } from '../src/lib/settings/state.svelte';

beforeEach(() => {
  tourState.active = false;
  tourState.currentStep = 0;
  settingsOpen.value = { tab: 'general' };
});

describe('GeneralPanel', () => {
  test('renders the Replay onboarding tour button', () => {
    render(GeneralPanel);
    expect(screen.getByRole('button', { name: /replay onboarding tour/i })).toBeTruthy();
  });

  test('clicking Replay activates the tour AND closes Settings', async () => {
    render(GeneralPanel);
    await fireEvent.click(screen.getByRole('button', { name: /replay onboarding tour/i }));
    expect(tourState.active).toBe(true);
    expect(tourState.currentStep).toBe(0);
    expect(settingsOpen.value).toBe(null);
  });

  test('Replay does NOT call appSettingsMarkTourCompleted', async () => {
    render(GeneralPanel);
    await fireEvent.click(screen.getByRole('button', { name: /replay onboarding tour/i }));
    expect(appSettingsMarkTourCompleted).not.toHaveBeenCalled();
  });
});

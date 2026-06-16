// tests/help-panel.test.ts
import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, test } from 'vitest';
import { tourState } from '../src/lib/onboarding/state.svelte';
import HelpPanel from '../src/lib/settings/HelpPanel.svelte';
import { settingsOpen } from '../src/lib/settings/state.svelte';

beforeEach(() => {
  tourState.active = false;
  tourState.currentStep = 0;
  settingsOpen.value = { tab: 'help' };
});

describe('HelpPanel', () => {
  test('renders the tip-level selector', () => {
    render(HelpPanel);
    expect(screen.getByTestId('tip-level-select')).toBeTruthy();
  });

  test('renders the Replay onboarding tour button', () => {
    render(HelpPanel);
    expect(screen.getByRole('button', { name: /replay onboarding tour/i })).toBeTruthy();
  });

  test('clicking Replay activates the tour AND closes Settings', async () => {
    render(HelpPanel);
    await fireEvent.click(screen.getByRole('button', { name: /replay onboarding tour/i }));
    expect(tourState.active).toBe(true);
    expect(tourState.currentStep).toBe(0);
    expect(settingsOpen.value).toBe(null);
  });
});

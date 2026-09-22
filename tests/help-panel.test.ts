// tests/help-panel.test.ts
import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, test } from 'vitest';
import { tourState } from '../src/lib/onboarding/state.svelte';
import HelpPanel from '../src/lib/settings/HelpPanel.svelte';
import { settingsOpen, settingsSearchFocus } from '../src/lib/settings/state.svelte';

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

  test('clicking Replay drops a pending jump with the modal — nothing flashes on the next open', async () => {
    // Not one of Help's own anchors: those would be consumed on mount.
    settingsSearchFocus.value = 'storage.cache';
    render(HelpPanel);
    await fireEvent.click(screen.getByRole('button', { name: /replay onboarding tour/i }));
    expect(settingsOpen.value).toBe(null);
    expect(settingsSearchFocus.value).toBe(null);
  });
});

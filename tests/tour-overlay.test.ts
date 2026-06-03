import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, test, vi } from 'vitest';

const appSettingsGet = vi.fn();
const appSettingsMarkTourCompleted = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: (...args: unknown[]) => appSettingsGet(...args),
    appSettingsMarkTourCompleted: (...args: unknown[]) => appSettingsMarkTourCompleted(...args),
  },
}));

import {
  ACCOUNT_STEP_INDEX,
  TOTAL_STEPS,
  TOUR_VERSION,
  tourState,
} from '../src/lib/onboarding/state.svelte';
import TourOverlay from '../src/lib/onboarding/TourOverlay.svelte';

beforeEach(() => {
  tourState.active = true;
  tourState.currentStep = 0;
  tourState.contextual = false;
  appSettingsMarkTourCompleted.mockResolvedValue({ status: 'ok', data: null });
});

describe('TourOverlay', () => {
  test('renders the welcome step on initial active state', () => {
    render(TourOverlay);
    expect(screen.getByText(/Welcome to Lucerna/i)).toBeTruthy();
    expect(screen.getByText(/Step 1 of 7/i)).toBeTruthy();
  });

  test('Next button advances currentStep', async () => {
    render(TourOverlay);
    await fireEvent.click(screen.getByRole('button', { name: /next/i }));
    expect(tourState.currentStep).toBe(1);
  });

  test('Back button is disabled on step 1', () => {
    render(TourOverlay);
    const back = screen.getByRole('button', {
      name: /back/i,
    }) as HTMLButtonElement;
    expect(back.disabled).toBe(true);
  });

  test('Skip button calls finishOrSkip + clears active', async () => {
    render(TourOverlay);
    await fireEvent.click(screen.getByRole('button', { name: /skip/i }));
    expect(appSettingsMarkTourCompleted).toHaveBeenCalledWith(TOUR_VERSION);
    expect(tourState.active).toBe(false);
  });

  test('On step 6, Next is replaced by Finish and clears active', async () => {
    tourState.currentStep = TOTAL_STEPS - 1;
    render(TourOverlay);
    const finish = screen.getByRole('button', { name: /finish/i });
    await fireEvent.click(finish);
    expect(appSettingsMarkTourCompleted).toHaveBeenCalledWith(TOUR_VERSION);
    expect(tourState.active).toBe(false);
  });

  test('Escape key triggers Skip behaviour', async () => {
    render(TourOverlay);
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(appSettingsMarkTourCompleted).toHaveBeenCalledWith(TOUR_VERSION);
  });

  test('Sets data-tour-active attribute on body while active', () => {
    render(TourOverlay);
    expect(document.body.getAttribute('data-tour-active')).toBe('true');
  });

  test('Does not render when tourState.active is false', () => {
    tourState.active = false;
    render(TourOverlay);
    expect(screen.queryByText(/Welcome to Lucerna/i)).toBeNull();
  });

  test('dialog uses aria-labelledby pointing at the title', () => {
    render(TourOverlay);
    const dialog = screen.getByRole('dialog');
    const labelledBy = dialog.getAttribute('aria-labelledby');
    expect(labelledBy).toBeTruthy();
    const title = document.getElementById(labelledBy as string);
    expect(title?.textContent).toMatch(/Welcome to Lucerna/i);
  });

  describe('contextual account hint mode', () => {
    beforeEach(() => {
      // This file's top-level beforeEach does not reset the persistence mock,
      // and earlier full-tour tests call it — clear so "not called" assertions
      // measure only this test's behaviour.
      appSettingsMarkTourCompleted.mockClear();
      tourState.contextual = true;
      tourState.currentStep = ACCOUNT_STEP_INDEX;
    });

    test('hides the step counter and Back/Skip/Next controls', () => {
      render(TourOverlay);
      expect(screen.queryByText(/Step \d+ of \d+/i)).toBeNull();
      expect(screen.queryByRole('button', { name: /back/i })).toBeNull();
      expect(screen.queryByRole('button', { name: /skip/i })).toBeNull();
      expect(screen.queryByRole('button', { name: /next/i })).toBeNull();
    });

    test('shows a single "Got it" button that closes without persisting', async () => {
      render(TourOverlay);
      const gotIt = screen.getByRole('button', { name: /got it/i });
      await fireEvent.click(gotIt);
      expect(tourState.active).toBe(false);
      expect(tourState.contextual).toBe(false);
      expect(appSettingsMarkTourCompleted).not.toHaveBeenCalled();
    });

    test('Escape closes the hint without persisting tour completion', async () => {
      render(TourOverlay);
      await fireEvent.keyDown(window, { key: 'Escape' });
      expect(tourState.active).toBe(false);
      expect(tourState.contextual).toBe(false);
      expect(appSettingsMarkTourCompleted).not.toHaveBeenCalled();
    });
  });

  test('Tab on the last focusable wraps to the first (focus trap)', async () => {
    render(TourOverlay);
    const buttons = screen.getAllByRole('button');
    const last = buttons[buttons.length - 1];
    last.focus();
    expect(document.activeElement).toBe(last);
    await fireEvent.keyDown(window, { key: 'Tab' });
    // Focus should have wrapped away from the last element.
    expect(document.activeElement).not.toBe(last);
  });
});

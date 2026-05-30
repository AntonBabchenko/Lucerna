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

import { TOTAL_STEPS, TOUR_VERSION, tourState } from '../src/lib/onboarding/state.svelte';
import TourOverlay from '../src/lib/onboarding/TourOverlay.svelte';

beforeEach(() => {
  tourState.active = true;
  tourState.currentStep = 0;
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

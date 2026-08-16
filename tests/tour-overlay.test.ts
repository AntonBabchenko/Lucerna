import { fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
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
  test('step 0 is the Basic/Advanced chooser with no Back/Skip/Next controls', () => {
    render(TourOverlay);
    expect(screen.getByText(/How much detail/i)).toBeTruthy();
    expect(screen.queryByRole('button', { name: /back/i })).toBeNull();
    expect(screen.queryByRole('button', { name: /skip/i })).toBeNull();
    expect(screen.queryByRole('button', { name: /next/i })).toBeNull();
  });

  test('renders the welcome step after the chooser', () => {
    tourState.currentStep = 1;
    render(TourOverlay);
    expect(screen.getByText(/Welcome to Lucerna/i)).toBeTruthy();
    expect(screen.getByText(/Step 2 of 8/i)).toBeTruthy();
  });

  test('Next button advances currentStep', async () => {
    tourState.currentStep = 1;
    render(TourOverlay);
    await fireEvent.click(screen.getByRole('button', { name: /next/i }));
    expect(tourState.currentStep).toBe(2);
  });

  test('Back button is enabled on the welcome step (can return to the chooser)', () => {
    tourState.currentStep = 1;
    render(TourOverlay);
    const back = screen.getByRole('button', { name: /back/i }) as HTMLButtonElement;
    expect(back.disabled).toBe(false);
  });

  test('Skip button calls finishOrSkip + clears active', async () => {
    tourState.currentStep = 1;
    render(TourOverlay);
    await fireEvent.click(screen.getByRole('button', { name: /skip/i }));
    expect(appSettingsMarkTourCompleted).toHaveBeenCalledWith(TOUR_VERSION);
    expect(tourState.active).toBe(false);
  });

  test('Skip button shares the btn-sm size of its row siblings (Back/Next)', () => {
    tourState.currentStep = 1;
    render(TourOverlay);
    const skip = screen.getByRole('button', { name: /skip/i });
    const next = screen.getByRole('button', { name: /next/i });
    // Regression: Skip used to be a bare `btn-tertiary` with no size class, so
    // it had no padding and a different font-size, breaking the footer row.
    expect(skip.classList.contains('btn-sm')).toBe(true);
    expect(next.classList.contains('btn-sm')).toBe(true);
  });

  test('Skip button never wraps to a second line (whitespace-nowrap)', () => {
    tourState.currentStep = 1;
    render(TourOverlay);
    const skip = screen.getByRole('button', { name: /skip/i });
    // Regression: a long label wrapped to two lines in the narrow popover,
    // making the footer row uneven. Keep it on one line.
    expect(skip.classList.contains('whitespace-nowrap')).toBe(true);
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
    tourState.currentStep = 1;
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

  // Regression: the popover carried a bare `z-50` and the spotlight/scrim
  // carried no z-index at all. Modals share that exact tier (--z-modal: 50)
  // and stack by DOM order, and <TourOverlay /> is mounted in +page.svelte
  // BEFORE QuickJoinDialog, PreflightGateDialog, the launch-warning
  // ConfirmDialog, ExportPackDialog and the rest — so any dialog opened during
  // the welcome tour painted straight over it. The tour tier exists for this
  // (--z-tour: 100 / --z-tour-popover: 101) and ContextualTour already uses it.
  test('paints on the tour tier, not the modal tier', () => {
    tourState.currentStep = 1;
    render(TourOverlay);
    const dialog = screen.getByRole('dialog');
    expect(dialog.className).toContain('z-[var(--z-tour-popover)]');
    expect(dialog.classList.contains('z-50')).toBe(false);
    expect(screen.getByTestId('tour-scrim').className).toContain('z-[var(--z-tour)]');
  });

  test('the spotlight sits on the tour tier too', () => {
    // STEPS[2] (sign-in) targets '[data-tour="account-section"]'; happy-dom
    // gives it a 0x0 rect, which TourOverlay still treats as a spotlight
    // (unlike ContextualTour, which centres instead).
    const target = document.createElement('div');
    target.setAttribute('data-tour', 'account-section');
    document.body.appendChild(target);
    tourState.currentStep = 2;
    render(TourOverlay);
    expect(screen.getByTestId('tour-spotlight').className).toContain('z-[var(--z-tour)]');
    target.remove();
  });

  test('the wrapper creates no stacking context that would trap the tiers', () => {
    // A z-*/opacity-*/transform class on `.tour-overlay` would make it the
    // children's containing stacking context, and the whole tour would drop
    // back to the wrapper's own (auto) level — silently undoing the fix above.
    render(TourOverlay);
    const wrapper = document.querySelector('.tour-overlay') as HTMLElement;
    expect(wrapper.className.trim()).toBe('tour-overlay');
  });

  test('the spotlight animates opacity only — never its geometry', () => {
    const target = document.createElement('div');
    target.setAttribute('data-tour', 'account-section');
    document.body.appendChild(target);
    tourState.currentStep = 2;
    render(TourOverlay);

    const spotlight = screen.getByTestId('tour-spotlight');
    // §12: transform / opacity / colour only. `transition-all` over the inline
    // left/top/width/height tweened four layout-bound properties AND repainted
    // the 9999px box-shadow that draws the dim, on every step change.
    expect(spotlight.className).not.toContain('transition-all');
    expect(spotlight.className).not.toContain('duration-200');
    // The reveal now comes from the shared .tour-spotlight opacity keyframe,
    // whose animated-property list is pinned in tests/intent/design-tokens.test.ts.
    expect(spotlight.classList.contains('tour-spotlight')).toBe(true);
    // Geometry still lands inline — the fix removes the tween, not the layout.
    expect(spotlight.getAttribute('style')).toContain('box-shadow: 0 0 0 9999px');
    target.remove();
  });

  test('a step change remounts the spotlight so the fade re-fires', async () => {
    // The {#key} is what makes the CSS mount-animation a per-step reveal. Node
    // identity is the only observable proof: a re-keyed block yields a NEW
    // element, an unkeyed one mutates the same node in place and never replays.
    const account = document.createElement('div');
    account.setAttribute('data-tour', 'account-section');
    const picker = document.createElement('div');
    picker.setAttribute('data-tour', 'instance-picker');
    document.body.append(account, picker);

    tourState.currentStep = 2;
    render(TourOverlay);
    const first = screen.getByTestId('tour-spotlight');

    tourState.currentStep = 3;
    // Two flushes: the first re-keys the block, the second lets the $effect's
    // updateRect() write the new rect back through the same markup.
    await tick();
    await tick();
    expect(screen.getByTestId('tour-spotlight')).not.toBe(first);

    account.remove();
    picker.remove();
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

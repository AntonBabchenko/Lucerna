import { fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(async () => ({ status: 'error', error: 'unused' })),
    appSettingsMarkTourCompleted: vi.fn(async () => ({ status: 'ok', data: null })),
  },
}));

import { hasSeen } from '../src/lib/onboarding/contextual-tours';
import { tourState } from '../src/lib/onboarding/state.svelte';
import { __resetLayers } from '../src/lib/ui/layer-stack.svelte';
import TourLayering from './fixtures/TourLayering.svelte';

const settle = async () => {
  await tick();
  await tick();
  await new Promise<void>((r) => queueMicrotask(r));
};
const tourCard = (id: string) =>
  document.querySelector(
    `[data-testid="contextual-tour-popover"][aria-labelledby="ctx-tour-title-${id}"]`,
  );
const anyTourPaint = () =>
  document.querySelector(
    '[data-testid="contextual-tour-popover"], [data-testid="contextual-tour-scrim"], [data-testid="contextual-tour-spotlight"]',
  );

describe('a contextual tour steps aside for whatever opens over it', () => {
  beforeEach(() => {
    localStorage.clear();
    __resetLayers();
    tourState.active = false;
    tourState.contextual = false;
    tourState.currentStep = 0;
  });

  it('an (i) opened during the tour is shown clear; Escape closes it and the tour returns on the same step', async () => {
    render(TourLayering);
    await settle();
    await fireEvent.click(document.querySelector<HTMLElement>('[data-tour-primary]')!);
    await settle();
    expect(screen.getByText(/Step 2 of 2/)).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Explain page' }));
    await settle();
    expect(screen.getByText('Page explanation.')).toBeTruthy();
    expect(anyTourPaint()).toBeNull();

    await fireEvent.keyDown(window, { key: 'Escape' });
    await settle();
    expect(screen.queryByText('Page explanation.')).toBeNull();
    expect(tourCard('addons')).not.toBeNull();
    expect(screen.getByText(/Step 2 of 2/)).toBeTruthy();
    expect(hasSeen('addons')).toBe(false);
    expect(document.activeElement?.hasAttribute('data-tour-primary')).toBe(true);
  });

  it('a dialog opened during the tour is shown clear; Escape closes only the dialog', async () => {
    render(TourLayering);
    await settle();
    await fireEvent.click(screen.getByRole('button', { name: 'Open dialog' }));
    await settle();
    expect(screen.getByRole('dialog', { name: 'Fixture dialog' })).toBeTruthy();
    expect(anyTourPaint()).toBeNull();

    await fireEvent.keyDown(window, { key: 'Escape' });
    await settle();
    expect(screen.queryByRole('dialog', { name: 'Fixture dialog' })).toBeNull();
    expect(tourCard('addons')).not.toBeNull();
    expect(hasSeen('addons')).toBe(false);
  });

  it("a dialog's own tour runs at once over it; the page tour resumes when the dialog closes", async () => {
    render(TourLayering, { props: { nestedTour: true } });
    await settle();
    await fireEvent.click(screen.getByRole('button', { name: 'Open dialog' }));
    await settle();
    expect(tourCard('l10n')).not.toBeNull();
    expect(tourCard('addons')).toBeNull();

    await fireEvent.keyDown(window, { key: 'Escape' }); // ends the dialog's tour
    await settle();
    expect(hasSeen('l10n')).toBe(true);
    expect(screen.getByRole('dialog', { name: 'Fixture dialog' })).toBeTruthy();

    await fireEvent.keyDown(window, { key: 'Escape' }); // closes the dialog
    await settle();
    expect(tourCard('addons')).not.toBeNull();
    expect(hasSeen('addons')).toBe(false);
  });

  it('a page tour that starts under an open dialog waits, then shows when it closes', async () => {
    render(TourLayering, { props: { dialogOpen: true } });
    await settle();
    expect(anyTourPaint()).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Close dialog' }));
    await settle();
    expect(tourCard('addons')).not.toBeNull();
  });

  it('a tour never shown is not burned when its surface goes away', async () => {
    render(TourLayering, { props: { dialogOpen: true } });
    await settle();
    await fireEvent.click(screen.getByRole('button', { name: 'Leave page' }));
    await settle();
    expect(hasSeen('addons')).toBe(false);
  });

  it('a tour that was shown, then stepped aside, is soft-skipped when its surface goes away', async () => {
    render(TourLayering);
    await settle();
    await fireEvent.click(screen.getByRole('button', { name: 'Open dialog' }));
    await settle();
    await fireEvent.click(screen.getByRole('button', { name: 'Leave page' }));
    await settle();
    expect(hasSeen('addons')).toBe(true);
  });

  it("a tour among a dialog's children finds its host and starts", async () => {
    render(TourLayering, { props: { pageTour: false, nestedTour: true, dialogOpen: true } });
    await settle();
    expect(tourCard('l10n')).not.toBeNull();
  });

  it('Escape closes an (i) inside a dialog and leaves the dialog open', async () => {
    render(TourLayering, { props: { pageTour: false, dialogOpen: true } });
    await settle();
    await fireEvent.click(screen.getByRole('button', { name: 'Explain dialog' }));
    await settle();
    expect(screen.getByText('Dialog explanation.')).toBeTruthy();
    await fireEvent.keyDown(window, { key: 'Escape' });
    await settle();
    expect(screen.queryByText('Dialog explanation.')).toBeNull();
    expect(screen.getByRole('dialog', { name: 'Fixture dialog' })).toBeTruthy();
  });
});

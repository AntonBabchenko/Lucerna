import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(async () => ({ status: 'error', error: 'unused' })),
    appSettingsMarkTourCompleted: vi.fn(async () => ({ status: 'ok', data: null })),
  },
}));

import ContextualTour from '../src/lib/onboarding/ContextualTour.svelte';
import { hasSeen, MANAGE_STEPS } from '../src/lib/onboarding/contextual-tours';
import { tourState } from '../src/lib/onboarding/state.svelte';
import TwoContextualTours from './fixtures/TwoContextualTours.svelte';

describe('ContextualTour interplay with the main tour', () => {
  beforeEach(() => {
    localStorage.clear();
    document.body.removeAttribute('data-ctx-tour-active');
    tourState.active = false;
    tourState.contextual = false;
    tourState.currentStep = 0;
  });

  it('yields (deactivates WITHOUT marking seen) when the main tour activates', async () => {
    render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    await tick();
    expect(screen.getByTestId('contextual-tour-popover')).toBeTruthy();

    tourState.active = true;
    await tick();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();
    expect(hasSeen('manage')).toBe(false);
  });

  it('unmount while the main tour is active does NOT mark seen (replay burn)', async () => {
    const { unmount } = render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    await tick();
    // Simulate replay: the flush that activates the main tour also tears the
    // host down (setMode('client') unmounts servers-mode hosts). The yield
    // effect never runs on a destroyed component; only onDestroy sees it.
    tourState.active = true;
    unmount();
    expect(hasSeen('manage')).toBe(false);
  });

  it('unmount mid-tour with no main tour stays a soft-skip (marks seen)', async () => {
    const { unmount } = render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    await tick();
    unmount();
    expect(hasSeen('manage')).toBe(true);
  });

  it('two tours mounting in the SAME flush cannot both activate', async () => {
    // The DOM flag is written by an $effect that runs AFTER onMount set `active`,
    // so a guard reading only <body> is blind to a sibling that activated in the
    // same flush: both popovers open, both dim, and one Escape closes both. The
    // claim must be taken synchronously at the moment `active` is set.
    // Two separate render() calls each flush on their own and so CANNOT
    // reproduce this — the fixture mounts both tours as children of one
    // component, which is a single flush.
    render(TwoContextualTours);
    await tick();
    expect(screen.queryAllByTestId('contextual-tour-popover')).toHaveLength(1);
    // The loser deferred rather than being consumed: it re-fires next mount.
    expect(hasSeen('logs')).toBe(false);
  });

  it('defers when another contextual tour is on screen', async () => {
    document.body.setAttribute('data-ctx-tour-active', 'true');
    render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    await tick();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();
    expect(hasSeen('manage')).toBe(false);
  });
});

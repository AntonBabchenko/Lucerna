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
import TourInReactiveHost from './fixtures/TourInReactiveHost.svelte';
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

  it('host torn down by the main tour activating does NOT mark seen (replay burn)', async () => {
    // The replay path: `replayTour()` calls setMode('client') and sets
    // tourState.active in ONE block, so the host unmounts in the same flush.
    // The fixture's {#if} is a block effect, which runs before user effects —
    // the tour is destroyed with its yield effect still pending, so onDestroy
    // sees `active === true` and only `!tourState.active` keeps it from
    // burning the flag replay just reset. See the fixture header for why an
    // imperative unmount() cannot reproduce this.
    render(TourInReactiveHost);
    await tick();
    expect(screen.getByTestId('contextual-tour-popover')).toBeTruthy();

    tourState.active = true;
    await tick();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();
    // onDestroy defers its decision one microtask (destroy-phase state reads
    // are stale), so settle the queue before reading the flag.
    await tick();
    expect(hasSeen('manage')).toBe(false);
  });

  it('unmount after the yield already drained does NOT mark seen', async () => {
    // Weaker sibling of the test above, and deliberately so: testing-library's
    // unmount() is flushSync(...), so the pending yield runs FIRST and clears
    // `active` before the destroy. This pins the yield-then-unmount sequence
    // only — it does NOT exercise onDestroy's !tourState.active guard (with
    // `active` already false, both guard variants agree).
    const { unmount } = render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    await tick();
    tourState.active = true;
    unmount();
    await tick();
    expect(hasSeen('manage')).toBe(false);
  });

  it('unmount mid-tour with no main tour stays a soft-skip (marks seen)', async () => {
    const { unmount } = render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    await tick();
    unmount();
    await tick();
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

  it('a DEFERRED tour is not burned when its host unmounts', async () => {
    // The `active` conjunct in the destroy check is load-bearing: a tour that
    // never showed (another tour held the claim) must stay armed for its next
    // mount, or "defer now, re-fire later" silently becomes "defer now, never
    // again". Note tourState.active is false throughout, so the main-tour half
    // of the check cannot cover this — only `active` can.
    const { unmount } = render(TwoContextualTours);
    await tick();
    expect(screen.queryAllByTestId('contextual-tour-popover')).toHaveLength(1);

    unmount();
    await tick();
    // The one that showed is soft-skipped; the one that deferred is untouched.
    expect(hasSeen('manage')).toBe(true);
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

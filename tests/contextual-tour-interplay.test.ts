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

  it('defers when another contextual tour is on screen', async () => {
    document.body.setAttribute('data-ctx-tour-active', 'true');
    render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    await tick();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();
    expect(hasSeen('manage')).toBe(false);
  });
});

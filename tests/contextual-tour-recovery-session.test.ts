import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(async () => ({ status: 'error', error: 'unused' })),
    appSettingsMarkTourCompleted: vi.fn(async () => ({ status: 'ok', data: null })),
    getDataLocation: vi.fn(async () => ({
      status: 'ok',
      data: {
        effective: 'C:\\Users\\u\\AppData\\Local\\com.lucerna.app\\recovery\\4242',
        configured: 'D:\\LucernaData',
        fell_back: true,
        fallback: { kind: 'root_missing' },
        default_dir: 'C:\\Default',
        relocation: { kind: 'idle' },
      },
    })),
  },
}));

import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
import { hasSeen, MANAGE_STEPS } from '$lib/onboarding/contextual-tours';
import { tourState } from '$lib/onboarding/state.svelte';
import { dataLocation } from '$lib/settings/data-location.svelte';

describe('ContextualTour in a recovery session', () => {
  beforeEach(() => {
    localStorage.clear();
    document.body.removeAttribute('data-ctx-tour-active');
    tourState.active = false;
    tourState.contextual = false;
    tourState.currentStep = 0;
  });

  it('does not open, and does not burn its seen flag', async () => {
    // A "create a server" tour over a disabled create button is noise; the surface stays
    // un-toured this visit and fires on the next one, once the launcher runs from its folder.
    await dataLocation.refresh();
    render(ContextualTour, { props: { id: 'manage', steps: MANAGE_STEPS } });
    await tick();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();
    expect(hasSeen('manage')).toBe(false);
  });
});

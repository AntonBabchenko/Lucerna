import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';

const setGeneral = vi.fn().mockResolvedValue({ status: 'ok', data: null });
vi.mock('$lib/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/ipc/bindings')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      appSettingsGet: vi.fn().mockResolvedValue({ status: 'ok', data: { general: {} } }),
      appSettingsSetGeneral: (g: unknown) => setGeneral(g),
    },
  };
});

import { explanationState } from '$lib/onboarding/explanation-level.svelte';
import { tourState } from '$lib/onboarding/state.svelte';
import TourOverlay from '$lib/onboarding/TourOverlay.svelte';

beforeEach(() => {
  locale.set('en');
  tourState.active = true;
  tourState.contextual = false;
  tourState.currentStep = 0;
  explanationState.level = 'basic';
  setGeneral.mockClear();
});

describe('tour chooser step', () => {
  it('selecting Advanced sets the level and advances to step 1', async () => {
    render(TourOverlay);
    await fireEvent.click(screen.getByText('Show the technical details'));
    expect(explanationState.level).toBe('advanced');
    expect(tourState.currentStep).toBe(1);
  });
});

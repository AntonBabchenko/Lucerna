// HELP-02: a replayed tour must not quietly steer the user back to Basic. On a
// replay the chooser shows — and focuses — the level that is current, when
// that level is known. First run and "could not tell" keep today's default.
import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { beforeEach, describe, expect, test, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(),
    appSettingsMarkTourCompleted: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    appSettingsPatchGeneral: vi.fn(),
  },
}));

import { replayTour, tourState } from '../src/lib/onboarding/state.svelte';
import TourOverlay from '../src/lib/onboarding/TourOverlay.svelte';
import { appSettings } from '../src/lib/settings/app-settings.svelte';

function known(level: 'basic' | 'advanced') {
  appSettings.loaded = {
    kind: 'ok',
    file: { general: { explanation_level: level } } as never,
  };
}

const basicBtn = () => screen.getByRole('button', { name: /^Basic/ });
const advancedBtn = () => screen.getByRole('button', { name: /^Advanced/ });

beforeEach(() => {
  tourState.active = true;
  tourState.currentStep = 0;
  tourState.contextual = false;
  tourState.replay = false;
  appSettings.loaded = { kind: 'pending' };
  appSettings.pending = [];
});

describe('the level chooser', () => {
  test('on a replay, the current level is the primary, pressed, focused choice — and says so', async () => {
    tourState.replay = true;
    known('advanced');
    render(TourOverlay);
    await tick();
    await tick();
    const adv = advancedBtn();
    expect(adv.className).toContain('btn-primary');
    expect(adv.getAttribute('aria-pressed')).toBe('true');
    expect(adv.textContent).toContain('Current');
    expect(document.activeElement).toBe(adv);
    const basic = basicBtn();
    expect(basic.className).toContain('btn-secondary');
    expect(basic.getAttribute('aria-pressed')).toBe('false');
    expect(basic.textContent).not.toContain('Current');
  });

  test('on a replay with Basic current, Basic is marked current', async () => {
    tourState.replay = true;
    known('basic');
    render(TourOverlay);
    await tick();
    expect(basicBtn().getAttribute('aria-pressed')).toBe('true');
    expect(basicBtn().textContent).toContain('Current');
  });

  test('on a replay whose level could not be read, nothing is claimed current', () => {
    tourState.replay = true;
    render(TourOverlay);
    expect(basicBtn().className).toContain('btn-primary');
    expect(screen.queryByText('Current')).toBeNull();
    expect(basicBtn().hasAttribute('aria-pressed')).toBe(false);
  });

  test('the first run keeps Basic as the default, with no "current" claim', () => {
    known('basic');
    render(TourOverlay);
    expect(basicBtn().className).toContain('btn-primary');
    expect(screen.queryByText('Current')).toBeNull();
  });

  test('Replay from Help marks the tour as a replay', () => {
    tourState.active = false;
    replayTour();
    expect(tourState.replay).toBe(true);
  });
});

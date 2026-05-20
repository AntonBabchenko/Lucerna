import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

const appSettingsGet = vi.fn();
const appSettingsMarkTourCompleted = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: (...args: unknown[]) => appSettingsGet(...args),
    appSettingsMarkTourCompleted: (...args: unknown[]) => appSettingsMarkTourCompleted(...args),
  },
}));

import {
  back,
  finishOrSkip,
  initOnboarding,
  next,
  replayTour,
  TOTAL_STEPS,
  TOUR_VERSION,
  tourState,
} from '../src/lib/onboarding/state.svelte';

beforeEach(() => {
  appSettingsGet.mockReset();
  appSettingsMarkTourCompleted.mockReset();
  // Reset the shared rune between tests.
  tourState.active = false;
  tourState.currentStep = 0;
});

describe('initOnboarding', () => {
  test('activates the tour when tour_completed_version is null', async () => {
    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { onboarding: { tour_completed_version: null } },
    });
    await initOnboarding();
    expect(tourState.active).toBe(true);
    expect(tourState.currentStep).toBe(0);
  });

  test('does NOT activate when version matches TOUR_VERSION', async () => {
    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { onboarding: { tour_completed_version: TOUR_VERSION } },
    });
    await initOnboarding();
    expect(tourState.active).toBe(false);
  });

  test('does NOT activate when IPC fails (silent fallback)', async () => {
    appSettingsGet.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: '<x>', details: 'boom' },
    });
    await initOnboarding();
    expect(tourState.active).toBe(false);
  });

  test('does NOT activate when version is OLDER than TOUR_VERSION', async () => {
    // Future-proofing — a user who finished a v0.4.0 tour still sees
    // the v0.5.0 tour. Asymmetric vs same-version case.
    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { onboarding: { tour_completed_version: '0.4.0' } },
    });
    await initOnboarding();
    expect(tourState.active).toBe(true);
  });
});

describe('next / back', () => {
  test('next increments currentStep up to TOTAL_STEPS-1', () => {
    tourState.currentStep = 0;
    next();
    expect(tourState.currentStep).toBe(1);
  });

  test('next is clamped at TOTAL_STEPS-1', () => {
    tourState.currentStep = TOTAL_STEPS - 1;
    next();
    expect(tourState.currentStep).toBe(TOTAL_STEPS - 1);
  });

  test('back decrements currentStep down to 0', () => {
    tourState.currentStep = 3;
    back();
    expect(tourState.currentStep).toBe(2);
  });

  test('back is clamped at 0', () => {
    tourState.currentStep = 0;
    back();
    expect(tourState.currentStep).toBe(0);
  });
});

describe('finishOrSkip', () => {
  test('calls IPC with TOUR_VERSION and clears active', async () => {
    appSettingsMarkTourCompleted.mockResolvedValue({ status: 'ok', data: null });
    tourState.active = true;
    tourState.currentStep = 4;
    await finishOrSkip();
    expect(appSettingsMarkTourCompleted).toHaveBeenCalledWith(TOUR_VERSION);
    expect(tourState.active).toBe(false);
  });
});

describe('replayTour', () => {
  test('sets active=true and currentStep=0 WITHOUT calling IPC', () => {
    appSettingsMarkTourCompleted.mockClear();
    tourState.active = false;
    tourState.currentStep = 5;
    replayTour();
    expect(tourState.active).toBe(true);
    expect(tourState.currentStep).toBe(0);
    expect(appSettingsMarkTourCompleted).not.toHaveBeenCalled();
  });
});

afterEach(() => {
  vi.clearAllMocks();
});

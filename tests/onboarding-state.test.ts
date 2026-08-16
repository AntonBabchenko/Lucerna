import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

const appSettingsGet = vi.fn();
const appSettingsMarkTourCompleted = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: (...args: unknown[]) => appSettingsGet(...args),
    appSettingsMarkTourCompleted: (...args: unknown[]) => appSettingsMarkTourCompleted(...args),
  },
}));

import { hasSeen, markSeen } from '../src/lib/onboarding/contextual-tours';
import {
  ACCOUNT_STEP_INDEX,
  back,
  closeHint,
  finishOrSkip,
  initOnboarding,
  next,
  replayTour,
  showAccountHint,
  TOTAL_STEPS,
  TOUR_VERSION,
  tourState,
} from '../src/lib/onboarding/state.svelte';
import { STEPS } from '../src/lib/onboarding/steps';
import { serversUi } from '../src/lib/servers/servers-ui.svelte';

beforeEach(() => {
  appSettingsGet.mockReset();
  appSettingsMarkTourCompleted.mockReset();
  // Reset the shared rune between tests.
  tourState.active = false;
  tourState.currentStep = 0;
  tourState.contextual = false;
  // Each mode-forcing test opts into servers mode explicitly; default to client
  // so a leaked servers mode from one test can't affect the next.
  serversUi.setMode('client');
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

  test('forces client mode when it activates the tour (main-tour anchors are client-only)', async () => {
    // The account section / instance picker / play button / modpacks anchors
    // render only in client mode, so a re-shown tour must yank a servers-mode
    // user back to client — otherwise it opens onto missing anchors.
    serversUi.setMode('servers');
    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { onboarding: { tour_completed_version: null } },
    });
    await initOnboarding();
    expect(serversUi.mode).toBe('client');
  });

  test('leaves servers mode untouched when the tour does NOT activate', async () => {
    serversUi.setMode('servers');
    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { onboarding: { tour_completed_version: TOUR_VERSION } },
    });
    await initOnboarding();
    expect(serversUi.mode).toBe('servers');
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

  test('reports whether the completion reached disk', async () => {
    appSettingsMarkTourCompleted.mockResolvedValue({ status: 'ok', data: null });
    expect(await finishOrSkip()).toBe(true);

    appSettingsMarkTourCompleted.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: '<app.json>', details: 'disk full' },
    });
    tourState.active = true;
    // Still closes — the caller decides what to say, not whether to leave.
    expect(await finishOrSkip()).toBe(false);
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

  test('also re-arms the per-surface contextual tours', () => {
    // Regression: replay used to restart only the main tour, leaving the
    // Logs/Manage/Modpacks/Worlds tours suppressed by their localStorage flags.
    localStorage.clear();
    markSeen('logs');
    markSeen('worlds');
    expect(hasSeen('logs')).toBe(true);

    replayTour();

    expect(hasSeen('logs')).toBe(false);
    expect(hasSeen('worlds')).toBe(false);
  });
});

describe('account hint (contextual reuse of the account step)', () => {
  test('ACCOUNT_STEP_INDEX points at the step targeting the account section', () => {
    expect(STEPS[ACCOUNT_STEP_INDEX]?.targetSelector).toBe('[data-tour="account-section"]');
  });

  test('showAccountHint activates the account step in contextual mode', () => {
    tourState.active = false;
    tourState.contextual = false;
    tourState.currentStep = 0;
    showAccountHint();
    expect(tourState.active).toBe(true);
    expect(tourState.contextual).toBe(true);
    expect(tourState.currentStep).toBe(ACCOUNT_STEP_INDEX);
  });

  test('showAccountHint fires even while a contextual tour is open (the ctx tour yields)', () => {
    document.body.setAttribute('data-ctx-tour-active', 'true');
    try {
      showAccountHint();
      expect(tourState.active).toBe(true);
      expect(tourState.contextual).toBe(true);
    } finally {
      document.body.removeAttribute('data-ctx-tour-active');
    }
  });

  test('showAccountHint forces client mode so the account anchor is present', () => {
    // The account section renders only in client mode; the hint spotlights
    // [data-tour="account-section"], so it must switch modes first.
    serversUi.setMode('servers');
    showAccountHint();
    expect(serversUi.mode).toBe('client');
  });

  test('closeHint clears active + contextual WITHOUT persisting tour completion', () => {
    appSettingsMarkTourCompleted.mockClear();
    showAccountHint();
    closeHint();
    expect(tourState.active).toBe(false);
    expect(tourState.contextual).toBe(false);
    expect(appSettingsMarkTourCompleted).not.toHaveBeenCalled();
  });
});

afterEach(() => {
  vi.clearAllMocks();
});

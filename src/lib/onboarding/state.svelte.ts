// Cross-component state for the v0.5.0 onboarding tour. Pattern is
// the same `.svelte.ts` rune module idiom as `settings/state.svelte.ts`
// (settingsOpen, cfKeyVersion, modBrowserNav from sub-3).
//
// The tour is auto-triggered by `initOnboarding` on +page mount when
// the user has not completed/skipped the v0.5.0 tour. `finishOrSkip`
// persists "I've seen 0.5.0" via IPC; `replayTour` is the Settings
// "Replay" path and intentionally does NOT touch persistence.

import { commands } from '$lib/ipc/bindings';
import { resetAllContextualTours } from './contextual-tours';
import { STEPS } from './steps';

export const TOUR_VERSION = '0.5.0';
// Derived from STEPS so adding/removing a step can never desync the
// clamp logic in next()/back() from the actual step count.
export const TOTAL_STEPS = STEPS.length;

export const tourState = $state<{ active: boolean; currentStep: number }>({
  active: false,
  currentStep: 0,
});

export async function initOnboarding(): Promise<void> {
  const r = await commands.appSettingsGet();
  if (r.status !== 'ok') return;
  if (r.data.onboarding.tour_completed_version !== TOUR_VERSION) {
    tourState.active = true;
    tourState.currentStep = 0;
  }
}

export function next(): void {
  if (tourState.currentStep < TOTAL_STEPS - 1) tourState.currentStep++;
}

export function back(): void {
  if (tourState.currentStep > 0) tourState.currentStep--;
}

export async function finishOrSkip(): Promise<void> {
  await commands.appSettingsMarkTourCompleted(TOUR_VERSION);
  tourState.active = false;
}

export function replayTour(): void {
  // Replay restarts the main tour AND re-arms the per-surface contextual
  // tours — otherwise the Logs/Manage/Modpacks/Worlds tours stay suppressed
  // by their localStorage flags and never reappear.
  resetAllContextualTours();
  tourState.currentStep = 0;
  tourState.active = true;
}

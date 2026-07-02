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

// Bumped 0.5.0 -> 0.6.0: PR #227 materially rewrote the main tour (Quick-Play,
// account discovery, worlds-import steps) without a version bump, so users who
// completed the old tour would never see the new onboarding. Bumping re-shows
// the improved tour once to existing users.
export const TOUR_VERSION = '0.6.0';
// Derived from STEPS so adding/removing a step can never desync the
// clamp logic in next()/back() from the actual step count.
export const TOTAL_STEPS = STEPS.length;

// Index of the ACCOUNT-section step, resolved from STEPS (not hard-coded) so a
// step reorder can't desync the on-demand account hint from the real step.
export const ACCOUNT_STEP_INDEX = STEPS.findIndex(
  (s) => s.targetSelector === '[data-tour="account-section"]',
);

// `contextual` flags the one-off "you need an account" hint: it reuses the
// account step's spotlight + copy but, unlike the full tour, hides the
// Step-X-of-Y counter and Back/Skip/Next controls and does NOT persist tour
// completion when dismissed.
export const tourState = $state<{ active: boolean; currentStep: number; contextual: boolean }>({
  active: false,
  currentStep: 0,
  contextual: false,
});

export async function initOnboarding(): Promise<void> {
  const r = await commands.appSettingsGet();
  if (r.status !== 'ok') return;
  if (r.data.onboarding.tour_completed_version !== TOUR_VERSION) {
    tourState.active = true;
    tourState.contextual = false;
    tourState.currentStep = 0;
  }
}

/** Show the on-demand account hint: reuse the account step's spotlight + copy
 *  in contextual mode (no tour chrome, no completion persistence). Triggered
 *  when Play is clicked with no active account. */
export function showAccountHint(): void {
  // Defensive: if a step reorder/rename ever desyncs the selector, findIndex
  // returns -1 and STEPS[-1] is undefined — rendering would throw. Fail silent
  // rather than crash the launch flow. (A test asserts the index resolves.)
  if (ACCOUNT_STEP_INDEX === -1) return;
  // Mutually exclude with contextual surface tours: opening the account hint on
  // top of a running contextual tour would freeze its popover (two overlays
  // fighting over the pointer-events kill). ContextualTour sets this attribute
  // while active; skip the hint until it closes.
  if (typeof document !== 'undefined' && document.body.hasAttribute('data-ctx-tour-active')) return;
  tourState.contextual = true;
  tourState.currentStep = ACCOUNT_STEP_INDEX;
  tourState.active = true;
}

/** Dismiss the contextual account hint without touching onboarding completion
 *  (the "Got it" button or Esc). */
export function closeHint(): void {
  tourState.active = false;
  tourState.contextual = false;
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
  tourState.contextual = false;
  tourState.active = true;
}

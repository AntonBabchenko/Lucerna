// Single owner of one fact: which contextual tour, if any, currently owns the
// screen (see ContextualTour.svelte).
//
// The fact needs two representations and they must never drift apart:
//   - a module-scope id, taken SYNCHRONOUSLY at the moment a tour activates.
//     The DOM attribute below cannot serve as the claim on its own: it used to
//     be written by an $effect that runs after the activating onMount, so two
//     tours mounting in the SAME flush (sibling hosts, or a modal whose tour
//     opens alongside a tab's) would both read an empty <body> and both
//     activate — two dims, and one Escape answered by both window handlers.
//   - `body[data-ctx-tour-active]`, which is the only form `Modal.svelte` (route
//     Escape to the tour instead of closing itself) and `trap-focus.ts` (yield
//     initial focus and Tab to the tour) can read; module state is invisible to
//     them.
//
// Both are written here, together, so no caller can set one and forget the
// other. Either half left stale is a session-long fault: a stale id suppresses
// every later contextual tour (there is no other reset path), and a stale
// attribute swallows every modal's Escape and every modal's focus trap.

import type { ContextualTourId } from './contextual-tours';

const ATTR = 'data-ctx-tour-active';

let activeTourId: ContextualTourId | null = null;

/** Whether any contextual tour currently owns the screen. */
export function isPresent(): boolean {
  return activeTourId !== null || document.body.hasAttribute(ATTR);
}

/**
 * Take the screen for `id`. Returns false when a tour already holds it, in
 * which case the caller must DEFER — stay un-toured this visit and re-fire on
 * its next mount — rather than opening a second overlay.
 */
export function claimPresence(id: ContextualTourId): boolean {
  if (isPresent()) return false;
  activeTourId = id;
  document.body.setAttribute(ATTR, 'true');
  return true;
}

/**
 * Give the screen back. Id-keyed and idempotent, so every "the tour ended" path
 * can call it: a caller that never claimed (or whose claim has already been
 * released and re-taken by another instance of the same id) cannot clear
 * somebody else's claim.
 */
export function releasePresence(id: ContextualTourId): void {
  if (activeTourId !== id) return;
  activeTourId = null;
  document.body.removeAttribute(ATTR);
}

/**
 * Test seam: drop any claim and its attribute. Production code must not call
 * this — a tour that is still on screen would be left running with the screen
 * marked free.
 */
export function __resetPresence(): void {
  activeTourId = null;
  document.body.removeAttribute(ATTR);
}

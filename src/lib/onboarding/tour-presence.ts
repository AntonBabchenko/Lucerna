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

import { whatsNewState } from '$lib/changelog/whats-new.svelte';
import type { ContextualTourId } from './contextual-tours';
import { tourState } from './state.svelte';

const ATTR = 'data-ctx-tour-active';

let activeTourId: ContextualTourId | null = null;

/**
 * Whether a surface OTHER than a contextual tour owns the screen right now —
 * the main onboarding tour, or the post-update changelog dialog.
 *
 * Two callers, and the second is the one that is easy to forget:
 *   - a host's mount gate, so a passive hint never opens on top of either. The
 *     changelog matters because it and the `overview` tour both arrive at
 *     startup on the default tab, and the dialog is `--z-modal: 50` against the
 *     contextual dim's `--z-tour: 100`: a tour left running paints its scrim
 *     over the changelog the user just clicked to read, and `Modal` hands their
 *     first Escape to the tour instead of closing the dialog.
 *   - ContextualTour's destroy guard. Yielding is implemented by the gate
 *     dropping the block, which routes through the same onDestroy whose job is
 *     to burn a tour whose host went away. Without this check, reading the
 *     changelog once would burn the tour for every future launch. Suppressed
 *     is not dismissed.
 *
 * Read it from a template (reactive: it reads `$state` during render) or from a
 * microtask after the destroying batch (honest: past Svelte's `old_values`).
 * Reading it inside a destroy phase would lie — see ContextualTour's onDestroy.
 */
export function screenOwnedElsewhere(): boolean {
  return tourState.active || whatsNewState.entries !== null;
}

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

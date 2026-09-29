// Which surface OTHER than a contextual tour owns the screen right now.
//
// Which contextual tour is on screen is not tracked here any more: the layer
// stack ($lib/ui/layer-stack.svelte.ts) owns that. A contextual tour is an entry
// in it, shown only while it is the top layer, and the stack is the single
// representation Modal (Escape) and trapFocus (focus) read — there is no body
// attribute to drift out of step with it.

import { whatsNewState } from '$lib/changelog/whats-new.svelte';
import { closeAskState } from '$lib/close/close-ask.svelte';
import { tourState } from './state.svelte';

/**
 * Whether a surface OTHER than a contextual tour owns the screen right now —
 * the main onboarding tour, the post-update changelog dialog, or the window's
 * close question (a tour over it would take the Escape meant for Cancel).
 *
 * Two callers, and the second is the one that is easy to forget:
 *   - a host's mount gate, so a passive hint does not run while one of them is
 *     up. The changelog matters because it and the `overview` tour both arrive
 *     at startup on the default tab, and the user clicked for the changelog:
 *     the hint yields to it and comes back after.
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
  return tourState.active || whatsNewState.entries !== null || closeAskState.open;
}

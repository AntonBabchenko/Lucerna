<script lang="ts">
  // One-shot tour overlay for a single surface. Mount inside the
  // host tab, or among the children of the host Modal (which then hosts it in
  // the layer stack). Auto-fires on first visit, then localStorage-persists
  // dismissed so it never returns. Mirrors TourOverlay's spotlight + popover
  // chrome; intentionally separate to keep main-tour state isolated.
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import type { TourStep } from './steps';
  import { hasSeen, markSeen, type ContextualTourId } from './contextual-tours';
  import { explanationState } from './explanation-level.svelte';
  import { explainKey } from './explanation-keys';
  import { popoverStyle } from './tour-placement';
  import { tourState } from './state.svelte';
  import { screenOwnedElsewhere } from './tour-presence';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { insertTour, isTopmost, layerHost, newLayerId } from '$lib/ui/layer-stack.svelte';

  let {
    id,
    steps,
    onStep = undefined,
  }: {
    id: ContextualTourId;
    steps: ReadonlyArray<TourStep>;
    /** Called each time a step shows (start, Next, Back, a return after stepping aside), before
     *  it is measured: what the step needs from the page is the host's to provide (the deps tour
     *  expands a mod's panel). Untracked — the host's state it reads is not this tour's. */
    onStep?: (index: number) => void;
  } = $props();

  let active = $state(false);
  let currentStep = $state(0);
  let rect = $state<DOMRect | null>(null);
  let popoverEl = $state<HTMLElement | null>(null);

  const PADDING = 6;

  // This tour's entry in the app's layer stack (layer-stack.svelte.ts). It sits
  // directly above its HOST — the Modal it is rendered inside, or the page —
  // and is shown only while it is the TOP layer: anything the user opens during
  // the tour (an (i), a menu, a dialog) goes on top, the tour steps aside with
  // its step kept, and comes back when that closes. Escape reaches it through
  // the layer router, i.e. only while it is on top.
  const layerId = newLayerId('contextual-tour');
  const host = layerHost();
  let releaseLayer: (() => void) | null = null;
  // Plain `let`, not $state: read by onDestroy's microtask (see there). Set
  // once the tour has actually been on screen.
  let everShown = false;

  const shown = $derived(active && isTopmost(layerId));

  // The layer is taken in onMount (below) and given back HERE, on this effect's
  // teardown — the one place every "the tour ended" path passes through:
  // finish() and the yield effect both clear `active`, and an unmount mid-tour
  // runs the teardown too. Svelte runs a teardown at most once per run, which
  // is what makes the release exactly-once-per-activation.
  //
  // Set-and-teardown, NOT if/else: this effect runs on every instance,
  // including one that deferred and never activated, and an `else` branch would
  // run a release this instance never took. A deferred instance registers no
  // teardown at all.
  $effect(() => {
    if (!active) return;
    return () => {
      releaseLayer?.();
      releaseLayer = null;
    };
  });

  // Yield to the main tour. Replay (Settings → Help) and a TOUR_VERSION-bump
  // re-show activate the main tour while a contextual popover can be up; two
  // live overlays freeze this one (body[data-tour-active] kills its pointer
  // events) and both window handlers answer one Escape. Deactivate WITHOUT
  // marking seen — replay just reset the flag, and the tour re-fires on the
  // next visit to its surface.
  $effect(() => {
    if (active && tourState.active) active = false;
  });

  onMount(() => {
    if (hasSeen(id)) return;
    // Not in a recovery session: a tour that teaches "create" over a disabled create button is
    // noise. The surface stays un-toured this visit and fires next time — the flag is not set.
    // These tours open on user navigation, well after the status has loaded, so the plain flag
    // (permissive while unknown) is enough here; the startup-only tour is stricter.
    if (dataLocation.fellBack) return;
    // Don't open on top of the main onboarding tour / account hint: two live
    // spotlights fight over focus and the pointer-events overlay, freezing the
    // contextual popover. Defer — the surface stays un-toured this visit and
    // re-fires next time (the "seen" flag is only set on finish).
    if (tourState.active) return;
    // Take a place in the layer stack above this tour's host, or defer if a
    // tour already runs at or above that level (two tours of one page). A
    // dialog opened OVER a running tour lies above it, so the dialog's own tour
    // is allowed and goes on top — the overview step's CTA opening the
    // translations editor, whose l10n tour then runs at once. Same deferral as
    // the main-tour case: this surface stays un-toured this visit and re-fires
    // on its next mount. The check-and-insert is one synchronous call, so two
    // tours mounting in the same flush cannot both get in.
    releaseLayer = insertTour(layerId, host, finish);
    if (!releaseLayer) return;
    active = true;
    const onResize = () => {
      if (shown) updateRect();
    };
    window.addEventListener('resize', onResize);
    window.addEventListener('scroll', onResize, true);
    return () => {
      window.removeEventListener('resize', onResize);
      window.removeEventListener('scroll', onResize, true);
    };
  });

  onDestroy(() => {
    // Nothing to release here: the effect above gives the layer back on
    // destroy (Svelte runs effect teardowns then too), and only for the
    // instance that actually took one. This callback decides one thing —
    // whether the id is burned.
    //
    // Host unmounted mid-tour: soft-skip so the tour doesn't re-fire on every
    // open — UNLESS another surface's arrival tore the host down, which is a
    // suppression and not a dismissal (the main tour's own activation, where
    // replay/startup call setMode('client') and set tourState.active in one
    // flush; or the post-update changelog dialog, which the `overview` host
    // yields to). `screenOwnedElsewhere()` owns that list.
    // Two Svelte facts dictate the shape of this check:
    //   1. the yield effect above cannot cover it — a destroyed component's
    //      pending $effects are discarded, so it never runs on that path;
    //   2. reading state HERE would lie. Svelte serves destroy-phase reads
    //      from `old_values`, i.e. the value from BEFORE the batch that
    //      destroyed us (`if (is_destroying_effect && old_values.has(signal))`
    //      in svelte/src/internal/client/runtime.js), so `tourState.active`
    //      reads false precisely when the main tour just switched it on.
    // One microtask lands after the batch, where both reads are honest. A tour
    // suppressed by replay was just reset by it and must stay armed.
    //
    // And only a tour the user has actually SEEN is burned: one inserted under
    // an open dialog that never reached the top was dismissed by nobody.
    // `everShown` is a plain variable, so this read is honest too.
    queueMicrotask(() => {
      if (active && everShown && !screenOwnedElsewhere()) markSeen(id);
    });
  });

  // On every appearance — activation, a return after stepping aside, a step
  // change — re-measure (the layout may have moved while hidden) and put focus
  // on the primary button unless the user is already inside the card: the top
  // layer owns focus.
  $effect(() => {
    void currentStep;
    if (!shown) return;
    everShown = true;
    const index = currentStep;
    untrack(() => onStep?.(index));
    updateRect();
    void tick().then(() => {
      if (!shown || !popoverEl) return;
      // After the tick: an anchor the host has just rendered for this step (from onStep) exists
      // only now.
      revealAnchor();
      updateRect();
      if (popoverEl.contains(document.activeElement)) return;
      popoverEl.querySelector<HTMLElement>('[data-tour-primary]')?.focus();
    });
  });

  // A `reveal` step's anchor may be anywhere in a long list. The browser's own "nearest" test
  // honours the scrollport's `scroll-padding` (the sticky toolbar and pager reserve themselves
  // there, `use:stickyEdge`), so an anchor under a sticky bar is revealed too — and one fully in
  // view does not move. `?.`: happy-dom has no scrollIntoView.
  function revealAnchor() {
    const step = steps[currentStep];
    if (!step?.reveal || !step.targetSelector) return;
    document
      .querySelector<HTMLElement>(step.targetSelector)
      ?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
  }

  function updateRect() {
    const sel = steps[currentStep]?.targetSelector;
    if (!sel) {
      rect = null;
      return;
    }
    const el = document.querySelector(sel);
    if (!el) {
      rect = null;
      return;
    }
    const r = (el as HTMLElement).getBoundingClientRect();
    // A CSS-hidden / zero-size anchor (e.g. the modpacks "filters" step reached
    // via an Imported deep-link, where the filter bar isn't rendered) yields a
    // 0×0 rect at 0,0. Drawing a spotlight there paints a tiny corner box, so
    // treat it as anchorless: centre the popover with no spotlight instead.
    rect = r.width > 0 && r.height > 0 ? r : null;
  }

  function next() {
    if (currentStep === steps.length - 1) {
      finish();
    } else {
      currentStep += 1;
    }
  }
  function back() {
    if (currentStep > 0) currentStep -= 1;
  }
  function finish() {
    markSeen(id);
    active = false;
  }
  // Tab only. Escape arrives through the layer router as `finish`, and only
  // while this tour is the top layer — a yielded or stepped-aside tour never
  // sees the Escape meant for whatever is on top.
  function onKeydown(e: KeyboardEvent) {
    if (!shown) return;
    if (e.key === 'Tab') {
      const items = popoverEl
        ? Array.from(popoverEl.querySelectorAll<HTMLElement>('button:not([disabled])'))
        : [];
      if (items.length === 0) return;
      const first = items[0];
      const last = items[items.length - 1];
      const activeEl = document.activeElement as HTMLElement | null;
      if (e.shiftKey && activeEl === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && activeEl === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }

  let step = $derived(steps[currentStep]);
  let isLast = $derived(currentStep === steps.length - 1);
  let isFirst = $derived(currentStep === 0);
  let level = $derived(explanationState.level);
</script>

<svelte:window onkeydown={onKeydown} />

{#if shown}
  {#if rect && step.targetSelector}
    <!-- Geometry is inline and JUMPS between steps rather than tweening: the dim
         IS this element's own 9999px box-shadow, so a transform would scale
         that shadow and the rounded corners with it and the highlight would
         change shape mid-transition. §12 allows transform / opacity / colour
         only, so the reveal is the shared `.tour-spotlight` opacity keyframe
         (app.css), replayed on mount by the {#key} below.

         Keyed on the STEP, not the rect: `updateRect()` also runs on every
         resize and every captured scroll event, and a rect key would restart
         the fade on each of those frames. -->
    {#key currentStep}
      <div
        class="tour-spotlight fixed pointer-events-none rounded-md z-[var(--z-tour)]"
        style="
          left: {rect.x - PADDING}px;
          top: {rect.y - PADDING}px;
          width: {rect.width + PADDING * 2}px;
          height: {rect.height + PADDING * 2}px;
          box-shadow: 0 0 0 9999px rgba(0,0,0,0.55);
        "
        data-testid="contextual-tour-spotlight"
      ></div>
    {/key}
  {:else}
    <div
      class="fixed inset-0 bg-black/55 z-[var(--z-tour)] pointer-events-none"
      data-testid="contextual-tour-scrim"
    ></div>
  {/if}

  <div
    bind:this={popoverEl}
    role="dialog"
    aria-modal="true"
    aria-labelledby="ctx-tour-title-{id}"
    class="fixed z-[var(--z-tour-popover)] bg-surface rounded shadow-xl p-4 w-[320px] max-w-[80vw]"
    style={popoverStyle(
      rect,
      step.anchor,
      typeof window !== 'undefined' ? window.innerWidth : 1280,
      typeof window !== 'undefined' ? window.innerHeight : 800,
    )}
    data-testid="contextual-tour-popover"
    data-ctx-tour-root
  >
    <div class="text-xs text-muted mb-1">
      {$t('onboarding.controls.stepOf', { current: currentStep + 1, total: steps.length })}
    </div>
    <h3 id="ctx-tour-title-{id}" class="font-semibold text-sm text-primary mb-2">
      {$t(explainKey(step.titleKey, level))}
    </h3>
    <p class="text-sm text-secondary mb-4">{$t(explainKey(step.bodyKey, level))}</p>
    <!-- Skip (dismiss the whole tour) sits alone on the far left, kept quiet so
         it can't be misclicked when reaching for the primary Next in the corner.
         Back + Next are paired on the right (Next isolated in the corner).
         Identical layout to TourOverlay so every tour surface matches. On the
         last step there is no Skip, so the pair pins right. -->
    <div class="flex items-center gap-2 {isLast ? 'justify-end' : 'justify-between'}">
      {#if !isLast}
        <button
          type="button"
          class="btn-ghost btn-sm inline-flex items-center whitespace-nowrap"
          onclick={finish}
        >
          {$t('onboarding.controls.skipContextual')}
        </button>
      {/if}
      <div class="flex items-center gap-2">
        <button
          type="button"
          class="btn-secondary btn-sm inline-flex items-center gap-1"
          disabled={isFirst}
          onclick={back}
        >
          <Icon name="arrowLeft" size={14} />
          {$t('onboarding.controls.back')}
        </button>
        <button
          type="button"
          data-tour-primary
          class="btn-primary btn-sm inline-flex items-center gap-1"
          onclick={next}
        >
          {#if isLast}
            {$t('onboarding.controls.gotIt')}
            <Icon name="success" size={14} />
          {:else}
            {$t('onboarding.controls.next')}
            <Icon name="arrowRight" size={14} />
          {/if}
        </button>
      </div>
    </div>
  </div>
{/if}

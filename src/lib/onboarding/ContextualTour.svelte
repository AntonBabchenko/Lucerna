<script lang="ts">
  // One-shot tour overlay for a single surface. Mount inside the
  // host modal/popover/tab. Auto-fires on first visit, then
  // localStorage-persists dismissed so it never returns. Mirrors
  // TourOverlay's spotlight + popover chrome; intentionally
  // separate to keep main-tour state isolated.
  import { onDestroy, onMount, tick } from 'svelte';
  import type { TourStep } from './steps';
  import { hasSeen, markSeen, type ContextualTourId } from './contextual-tours';
  import { explanationState } from './explanation-level.svelte';
  import { explainKey } from './explanation-keys';
  import { tourState } from './state.svelte';
  import { claimPresence, releasePresence, screenOwnedElsewhere } from './tour-presence';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';

  let { id, steps }: { id: ContextualTourId; steps: ReadonlyArray<TourStep> } = $props();

  let active = $state(false);
  let currentStep = $state(0);
  let rect = $state<DOMRect | null>(null);
  let popoverEl = $state<HTMLElement | null>(null);

  const POPOVER_WIDTH = 320;
  const MARGIN = 16;
  const PADDING = 6;

  // The claim is taken in onMount (below) and given back HERE, on this effect's
  // teardown — the one place every "the tour ended" path passes through:
  // finish() and the yield effect both clear `active`, and an unmount mid-tour
  // runs the teardown too. Svelte runs a teardown at most once per run, which
  // is what makes the release exactly-once-per-activation.
  //
  // Set-and-teardown, NOT if/else: this effect runs on every instance,
  // including one that deferred and never activated, and an `else` branch would
  // release a claim this instance never took — defeating the ctx-vs-ctx guard
  // in onMount. A deferred instance registers no teardown at all.
  $effect(() => {
    if (active) return () => releasePresence(id);
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
    // Don't open on top of the main onboarding tour / account hint: two live
    // spotlights fight over focus and the pointer-events overlay, freezing the
    // contextual popover. Defer — the surface stays un-toured this visit and
    // re-fires next time (the "seen" flag is only set on finish).
    if (tourState.active) return;
    // Take the screen, or defer if another contextual tour already holds it
    // (cross-surface chaining: e.g. the overview step's own CTA opens the
    // translations modal, which hosts the l10n tour). Same deferral as the
    // main-tour case — this surface stays un-toured this visit and re-fires on
    // its next mount. See tour-presence.ts for why the claim must be
    // synchronous here rather than inferred from the <body> flag.
    if (!claimPresence(id)) return;
    active = true;
    void tick().then(() => updateRect());
    const onResize = () => {
      if (active) updateRect();
    };
    window.addEventListener('resize', onResize);
    window.addEventListener('scroll', onResize, true);
    return () => {
      window.removeEventListener('resize', onResize);
      window.removeEventListener('scroll', onResize, true);
    };
  });

  onDestroy(() => {
    // Nothing to release here: the effect above hands the screen back on
    // destroy (Svelte runs effect teardowns then too), and only for the
    // instance that actually claimed it. This callback decides one thing —
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
    queueMicrotask(() => {
      if (active && !screenOwnedElsewhere()) markSeen(id);
    });
  });

  $effect(() => {
    void currentStep;
    if (active) updateRect();
  });

  $effect(() => {
    void currentStep;
    if (active) {
      void tick().then(() => {
        if (!popoverEl) return;
        if (popoverEl.contains(document.activeElement)) return;
        popoverEl.querySelector<HTMLElement>('[data-tour-primary]')?.focus();
      });
    }
  });

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
  function onKeydown(e: KeyboardEvent) {
    if (!active) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      finish();
      return;
    }
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

  function popoverStyle(r: DOMRect | null, anchor: string): string {
    if (!r) {
      return 'top:50%; left:50%; transform:translate(-50%,-50%);';
    }
    const vw = typeof window !== 'undefined' ? window.innerWidth : 1280;
    const vh = typeof window !== 'undefined' ? window.innerHeight : 800;
    if (anchor === 'right') {
      const leftPart =
        r.right + MARGIN + POPOVER_WIDTH + MARGIN <= vw
          ? `left:${r.right + MARGIN}px;`
          : `left:${Math.max(MARGIN, r.left - POPOVER_WIDTH - MARGIN)}px;`;
      const midY = r.top + r.height / 2;
      const vertical =
        midY > vh / 2 ? `bottom:${Math.max(MARGIN, vh - r.bottom)}px;` : `top:${r.top}px;`;
      return `${vertical} ${leftPart}`;
    }
    if (anchor === 'below') {
      let leftCoord = r.left;
      if (leftCoord + POPOVER_WIDTH + MARGIN > vw) {
        leftCoord = Math.max(MARGIN, vw - POPOVER_WIDTH - MARGIN);
      }
      // Flip above the anchor when there isn't room below it. A `below`-anchored
      // step near the viewport bottom (e.g. the manage-instances actions row)
      // would otherwise position the popover off the bottom edge — invisible,
      // leaving only a dimmed screen with no reachable controls.
      const POPOVER_HEIGHT_BUDGET = 220;
      const fitsBelow = r.bottom + 12 + POPOVER_HEIGHT_BUDGET <= vh;
      if (fitsBelow) {
        return `top:${r.bottom + 12}px; left:${leftCoord}px;`;
      }
      return `bottom:${Math.max(MARGIN, vh - r.top + 12)}px; left:${leftCoord}px;`;
    }
    return 'top:50%; left:50%; transform:translate(-50%,-50%);';
  }

  let step = $derived(steps[currentStep]);
  let isLast = $derived(currentStep === steps.length - 1);
  let isFirst = $derived(currentStep === 0);
  let level = $derived(explanationState.level);
</script>

<svelte:window onkeydown={onKeydown} />

{#if active}
  {#if rect && step.targetSelector}
    <div
      class="fixed pointer-events-none transition-all duration-200 rounded-md z-[var(--z-tour)]"
      style="
        left: {rect.x - PADDING}px;
        top: {rect.y - PADDING}px;
        width: {rect.width + PADDING * 2}px;
        height: {rect.height + PADDING * 2}px;
        box-shadow: 0 0 0 9999px rgba(0,0,0,0.55);
      "
    ></div>
  {:else}
    <div class="fixed inset-0 bg-black/55 z-[var(--z-tour)] pointer-events-none"></div>
  {/if}

  <div
    bind:this={popoverEl}
    role="dialog"
    aria-modal="true"
    aria-labelledby="ctx-tour-title-{id}"
    class="fixed z-[var(--z-tour-popover)] bg-surface rounded shadow-xl p-4 w-[320px] max-w-[80vw]"
    style={popoverStyle(rect, step.anchor)}
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

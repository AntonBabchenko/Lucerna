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

  // While a contextual tour is on screen, flag the body so the host Modal knows
  // to route Escape to the tour instead of closing itself. The tour is
  // deliberately NON-blocking (the dim is pointer-events:none) — an earlier
  // blocking variant could trap the user behind a mispositioned popover and
  // intercepted legitimate clicks, so we only coordinate Escape here.
  $effect(() => {
    if (active) {
      document.body.setAttribute('data-ctx-tour-active', 'true');
    } else {
      document.body.removeAttribute('data-ctx-tour-active');
    }
  });

  onMount(() => {
    if (hasSeen(id)) return;
    // Don't open on top of the main onboarding tour / account hint: two live
    // spotlights fight over focus and the pointer-events overlay, freezing the
    // contextual popover. Defer — the surface stays un-toured this visit and
    // re-fires next time (the "seen" flag is only set on finish).
    if (tourState.active) return;
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
    document.body.removeAttribute('data-ctx-tour-active');
    // If the host (modal/tab) unmounts mid-tour, treat it as a soft-skip so the
    // tour doesn't silently re-fire on every subsequent open. finish() already
    // clears `active`, so this only fires on an un-finished dismissal.
    if (active) markSeen(id);
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
      class="fixed pointer-events-none transition-all duration-200 rounded-md z-[100]"
      style="
        left: {rect.x - PADDING}px;
        top: {rect.y - PADDING}px;
        width: {rect.width + PADDING * 2}px;
        height: {rect.height + PADDING * 2}px;
        box-shadow: 0 0 0 9999px rgba(0,0,0,0.55);
      "
    ></div>
  {:else}
    <div class="fixed inset-0 bg-black/55 z-[100] pointer-events-none"></div>
  {/if}

  <div
    bind:this={popoverEl}
    role="dialog"
    aria-modal="true"
    aria-labelledby="ctx-tour-title-{id}"
    class="fixed z-[101] bg-surface rounded shadow-xl p-4 w-[320px] max-w-[80vw]"
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
    <!-- Back / Skip / Next are three direct children so justify-between gives
         Skip an equal gap from each neighbour (Skip sits centred, not jammed
         against the primary Next). Mirrors TourOverlay's main-tour row so the
         control layout is identical across every tour surface. -->
    <div class="flex items-center justify-between gap-2">
      <button
        type="button"
        class="btn-secondary btn-sm inline-flex items-center gap-1"
        disabled={isFirst}
        onclick={back}
      >
        <Icon name="arrowLeft" size={14} />
        {$t('onboarding.controls.back')}
      </button>
      {#if !isLast}
        <button
          type="button"
          class="btn-ghost btn-sm inline-flex items-center whitespace-nowrap"
          onclick={finish}
        >
          {$t('onboarding.controls.skipContextual')}
        </button>
      {/if}
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
{/if}

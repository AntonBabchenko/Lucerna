<script lang="ts">
  // 6-step spotlight overlay tour. Reads tourState + STEPS, drives
  // next / back / finish. The spotlight uses the box-shadow outset
  // trick to dim everything except a small rect around the target;
  // popover is anchored next to the rect with discrete positioning.
  //
  // While active, sets <body data-tour-active="true"> so a global CSS
  // rule disables pointer-events on the underlying UI. Esc = Skip.
  import { onMount, untrack, tick } from 'svelte';
  import { tourState, TOTAL_STEPS, next, back, finishOrSkip, closeHint } from './state.svelte';
  import { STEPS } from './steps';
  import { explanationState, setExplanationLevel } from './explanation-level.svelte';
  import { explainKey } from './explanation-keys';
  import { Icon } from '$lib/ui/icons';
  import type { ExplanationLevel } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';

  const PADDING = 6;

  let rect = $state<DOMRect | null>(null);
  let popoverEl = $state<HTMLElement | null>(null);

  $effect(() => {
    if (!tourState.active) {
      document.body.removeAttribute('data-tour-active');
      rect = null;
      return;
    }
    document.body.setAttribute('data-tour-active', 'true');
    untrack(() => updateRect());
  });

  $effect(() => {
    // Recompute rect when the step changes.
    void tourState.currentStep;
    if (tourState.active) updateRect();
  });

  $effect(() => {
    // Move focus into the popover when the tour opens or the step changes,
    // so keyboard users land on the primary action and the Tab trap applies.
    void tourState.currentStep;
    if (tourState.active) {
      void tick().then(() => {
        if (!popoverEl) return;
        // Don't steal focus if it is already somewhere inside the popover —
        // that means the user (or the Tab trap) put it there deliberately.
        if (popoverEl.contains(document.activeElement)) return;
        popoverEl.querySelector<HTMLElement>('[data-tour-primary]')?.focus();
      });
    }
  });

  function updateRect() {
    const sel = STEPS[tourState.currentStep]?.targetSelector;
    if (!sel) {
      rect = null;
      return;
    }
    const el = document.querySelector(sel);
    rect = el ? (el as HTMLElement).getBoundingClientRect() : null;
  }

  function onResize() {
    if (tourState.active) updateRect();
  }

  function focusables(): HTMLElement[] {
    if (!popoverEl) return [];
    return Array.from(popoverEl.querySelectorAll<HTMLElement>('button:not([disabled])'));
  }

  function onKeydown(e: KeyboardEvent) {
    if (!tourState.active) return;
    if (e.key === 'Escape') {
      // The contextual account hint dismisses without marking the onboarding
      // tour completed; the full tour's Esc is "Skip".
      if (tourState.contextual) closeHint();
      else void finishOrSkip();
      return;
    }
    if (e.key === 'Tab') {
      // Trap Tab focus inside the popover: pointer-events:none on the
      // background does not block keyboard activation, so without this a
      // user could Tab to and activate a background control.
      const items = focusables();
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

  onMount(() => {
    window.addEventListener('resize', onResize);
    window.addEventListener('scroll', onResize, true);
    return () => {
      window.removeEventListener('resize', onResize);
      window.removeEventListener('scroll', onResize, true);
      document.body.removeAttribute('data-tour-active');
    };
  });

  // Popover positioning per anchor with viewport-overflow clamping.
  // When the target sits in the bottom half of the viewport, anchor
  // the popover by its BOTTOM edge (sticking up from the target)
  // rather than its top — otherwise a target near the bottom (e.g.
  // sidebar's Browse modpacks for step 6) pushes the popover off the
  // screen. Same idea horizontally for `anchor: 'below'`.
  const POPOVER_WIDTH = 320;
  const MARGIN = 16;

  function popoverStyle(r: DOMRect | null, anchor: string): string {
    if (!r) {
      return 'top:50%; left:50%; transform:translate(-50%,-50%);';
    }
    const vw = typeof window !== 'undefined' ? window.innerWidth : 1280;
    const vh = typeof window !== 'undefined' ? window.innerHeight : 800;

    if (anchor === 'right') {
      // Horizontal: prefer right of target; fall back left if no room.
      let leftPart: string;
      if (r.right + MARGIN + POPOVER_WIDTH + MARGIN <= vw) {
        leftPart = `left:${r.right + MARGIN}px;`;
      } else {
        const fallbackLeft = Math.max(MARGIN, r.left - POPOVER_WIDTH - MARGIN);
        leftPart = `left:${fallbackLeft}px;`;
      }
      // Vertical: if target's vertical centre is in the bottom half of
      // the viewport, anchor the popover BOTTOM to the target bottom
      // (popover grows upward). Otherwise top-align as before.
      const targetMidY = r.top + r.height / 2;
      const verticalPart =
        targetMidY > vh / 2 ? `bottom:${Math.max(MARGIN, vh - r.bottom)}px;` : `top:${r.top}px;`;
      return `${verticalPart} ${leftPart}`;
    }

    if (anchor === 'below') {
      // Horizontal: keep popover inside the viewport when target is
      // near the right edge.
      let leftCoord = r.left;
      if (leftCoord + POPOVER_WIDTH + MARGIN > vw) {
        leftCoord = Math.max(MARGIN, vw - POPOVER_WIDTH - MARGIN);
      }
      return `top:${r.bottom + 12}px; left:${leftCoord}px;`;
    }

    return 'top:50%; left:50%; transform:translate(-50%,-50%);';
  }

  let step = $derived(STEPS[tourState.currentStep]);
  let isLast = $derived(tourState.currentStep === TOTAL_STEPS - 1);
  let isFirst = $derived(tourState.currentStep === 0);

  let level = $derived(explanationState.level);
  let isChooser = $derived(step?.kind === 'chooser');
  // Chooser uses its keys verbatim; all other steps resolve to the level variant.
  let titleKey = $derived(isChooser ? step.titleKey : explainKey(step.titleKey, level));
  let bodyKey = $derived(isChooser ? step.bodyKey : explainKey(step.bodyKey, level));

  function chooseLevel(l: ExplanationLevel) {
    void setExplanationLevel(l);
    next();
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if tourState.active}
  <div class="tour-overlay">
    {#if rect && step.targetSelector}
      <!-- Spotlight: small div at target rect, huge dark outset shadow
           darkens everything outside it. pointer-events:none. -->
      <div
        class="fixed pointer-events-none transition-all duration-200 rounded-md"
        style="
          left: {rect.x - PADDING}px;
          top: {rect.y - PADDING}px;
          width: {rect.width + PADDING * 2}px;
          height: {rect.height + PADDING * 2}px;
          box-shadow: 0 0 0 9999px rgba(0,0,0,0.55);
        "
      ></div>
    {:else}
      <div class="fixed inset-0 bg-black/55 pointer-events-none"></div>
    {/if}

    <div
      bind:this={popoverEl}
      role="dialog"
      aria-modal="true"
      aria-labelledby="tour-popover-title"
      class="fixed z-50 bg-surface rounded shadow-xl p-4 w-[320px] max-w-[80vw]"
      style={popoverStyle(rect, step.anchor)}
    >
      {#if !tourState.contextual}
        <div class="text-xs text-muted mb-1">
          {$t('onboarding.controls.stepOf', {
            current: tourState.currentStep + 1,
            total: TOTAL_STEPS,
          })}
        </div>
      {/if}
      <h3 id="tour-popover-title" class="font-semibold text-sm text-primary mb-2">
        {$t(titleKey)}
      </h3>
      <p class="text-sm text-secondary mb-4">{$t(bodyKey)}</p>
      {#if step.disclaimerKey}
        <p class="text-xs text-muted mt-3">{$t(step.disclaimerKey)}</p>
      {/if}
      {#if isChooser}
        <div class="flex flex-col gap-2">
          <button
            type="button"
            data-tour-primary
            class="btn-primary w-full text-center flex flex-col items-center py-2"
            onclick={() => chooseLevel('basic')}
          >
            <span class="font-medium">{$t('onboarding.chooser.basicLabel')}</span>
            <span class="text-xs opacity-80">{$t('onboarding.chooser.basicHint')}</span>
          </button>
          <button
            type="button"
            class="btn-secondary w-full text-center flex flex-col items-center py-2"
            onclick={() => chooseLevel('advanced')}
          >
            <span class="font-medium">{$t('onboarding.chooser.advancedLabel')}</span>
            <span class="text-xs opacity-80">{$t('onboarding.chooser.advancedHint')}</span>
          </button>
        </div>
      {:else if tourState.contextual}
        <!-- On-demand account hint: no tour navigation, just acknowledge. -->
        <div class="flex justify-end">
          <button
            type="button"
            data-tour-primary
            class="btn-primary btn-sm inline-flex items-center gap-1"
            onclick={closeHint}
          >
            {$t('onboarding.controls.gotIt')}
            <Icon name="success" size={14} />
          </button>
        </div>
      {:else}
        <div class="flex justify-between gap-2">
          <button
            type="button"
            class="btn-secondary btn-sm inline-flex items-center gap-1"
            disabled={isFirst}
            onclick={() => back()}
          >
            <Icon name="arrowLeft" size={14} />
            {$t('onboarding.controls.back')}
          </button>
          <div class="flex gap-2">
            {#if !isLast}
              <!-- Pull the ghost button's invisible trailing padding back so its
                   resting label sits a normal gap from Next, not px-3 + gap-2
                   adrift. The padding stays for the hover surface. -->
              <button
                type="button"
                class="btn-ghost btn-sm inline-flex items-center whitespace-nowrap -mr-2"
                onclick={() => void finishOrSkip()}
              >
                {$t('onboarding.controls.skip')}
              </button>
            {/if}
            <button
              type="button"
              data-tour-primary
              class="btn-primary btn-sm inline-flex items-center gap-1"
              onclick={() => (isLast ? void finishOrSkip() : next())}
            >
              {#if isLast}
                {$t('onboarding.controls.finish')}
                <Icon name="success" size={14} />
              {:else}
                {$t('onboarding.controls.next')}
                <Icon name="arrowRight" size={14} />
              {/if}
            </button>
          </div>
        </div>
      {/if}
    </div>
  </div>
{/if}

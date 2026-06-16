<script lang="ts">
  // One-shot tour overlay for a single surface. Mount inside the
  // host modal/popover/tab. Auto-fires on first visit, then
  // localStorage-persists dismissed so it never returns. Mirrors
  // TourOverlay's spotlight + popover chrome; intentionally
  // separate to keep main-tour state isolated.
  import { onMount, tick } from 'svelte';
  import type { TourStep } from './steps';
  import { hasSeen, markSeen, type ContextualTourId } from './contextual-tours';
  import { explanationState } from './explanation-level.svelte';
  import { explainKey } from './explanation-keys';
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

  onMount(() => {
    if (hasSeen(id)) return;
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
    rect = el ? (el as HTMLElement).getBoundingClientRect() : null;
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
      return `top:${r.bottom + 12}px; left:${leftCoord}px;`;
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
  >
    <div class="text-xs text-muted mb-1">
      {$t('onboarding.controls.stepOf', { current: currentStep + 1, total: steps.length })}
    </div>
    <h3 id="ctx-tour-title-{id}" class="font-semibold text-sm text-primary mb-2">
      {$t(explainKey(step.titleKey, level))}
    </h3>
    <p class="text-sm text-secondary mb-4">{$t(explainKey(step.bodyKey, level))}</p>
    <div class="flex justify-between gap-2">
      <button
        type="button"
        class="btn-secondary btn-sm inline-flex items-center gap-1"
        disabled={isFirst}
        onclick={back}
      >
        <Icon name="arrowLeft" size={14} />
        {$t('onboarding.controls.back')}
      </button>
      <div class="flex gap-2">
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
  </div>
{/if}

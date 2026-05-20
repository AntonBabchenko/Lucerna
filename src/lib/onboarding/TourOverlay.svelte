<script lang="ts">
  // 6-step spotlight overlay tour. Reads tourState + STEPS, drives
  // next / back / finish. The spotlight uses the box-shadow outset
  // trick to dim everything except a small rect around the target;
  // popover is anchored next to the rect with discrete positioning.
  //
  // While active, sets <body data-tour-active="true"> so a global CSS
  // rule disables pointer-events on the underlying UI. Esc = Skip.
  import { onMount, untrack, tick } from 'svelte';
  import {
    tourState,
    TOTAL_STEPS,
    next,
    back,
    finishOrSkip,
  } from './state.svelte';
  import { STEPS } from './steps';

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
        popoverEl
          .querySelector<HTMLElement>('[data-tour-primary]')
          ?.focus();
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
    return Array.from(
      popoverEl.querySelectorAll<HTMLElement>('button:not([disabled])'),
    );
  }

  function onKeydown(e: KeyboardEvent) {
    if (!tourState.active) return;
    if (e.key === 'Escape') {
      void finishOrSkip();
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

  // Popover positioning per anchor. Returns inline style string.
  function popoverStyle(r: DOMRect | null, anchor: string): string {
    if (!r) {
      return 'top:50%; left:50%; transform:translate(-50%,-50%);';
    }
    if (anchor === 'right') {
      return `top:${r.top}px; left:${r.right + 16}px;`;
    }
    if (anchor === 'below') {
      return `top:${r.bottom + 12}px; left:${r.left}px;`;
    }
    return 'top:50%; left:50%; transform:translate(-50%,-50%);';
  }

  let step = $derived(STEPS[tourState.currentStep]);
  let isLast = $derived(tourState.currentStep === TOTAL_STEPS - 1);
  let isFirst = $derived(tourState.currentStep === 0);
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
      class="fixed z-50 bg-white rounded shadow-xl p-4 w-[320px] max-w-[80vw]"
      style={popoverStyle(rect, step.anchor)}
    >
      <div class="text-xs text-neutral-500 mb-1">
        Step {tourState.currentStep + 1} of {TOTAL_STEPS}
      </div>
      <h3 id="tour-popover-title" class="font-semibold text-sm mb-2">
        {step.title}
      </h3>
      <p class="text-sm text-neutral-700 mb-4">{step.body}</p>
      <div class="flex justify-between gap-2">
        <button
          type="button"
          class="text-sm text-neutral-500 hover:text-neutral-800 disabled:opacity-30"
          disabled={isFirst}
          onclick={() => back()}
        >
          ← Back
        </button>
        <div class="flex gap-2">
          {#if !isLast}
            <button
              type="button"
              class="text-sm text-neutral-500 hover:text-neutral-800"
              onclick={() => void finishOrSkip()}
            >
              Skip tour
            </button>
          {/if}
          <button
            type="button"
            data-tour-primary
            class="text-sm px-3 py-1 rounded bg-blue-600 text-white hover:bg-blue-700"
            onclick={() => (isLast ? void finishOrSkip() : next())}
          >
            {isLast ? 'Finish ✓' : 'Next →'}
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}

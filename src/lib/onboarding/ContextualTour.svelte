<script module lang="ts">
  // Which contextual tour currently owns the screen, shared across every
  // instance of this component. The <body> attribute below cannot serve as the
  // claim: it is written by an $effect that runs AFTER onMount set `active`, so
  // two tours mounting in the SAME flush (sibling hosts, or a modal whose tour
  // opens alongside a tab's) would both read an empty <body> and both activate
  // — two dims, and one Escape answered by both window handlers. This claim is
  // taken synchronously, in onMount, at the moment `active` is set.
  //
  // The DOM attribute stays: `Modal.svelte` and `trap-focus.ts` read it, and
  // module state is invisible to them.
  let activeTourId: string | null = null;
</script>

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

  // True only for the instance that took the module-level claim. Deliberately
  // not `$state` — nothing renders from it.
  //
  // Belt-and-braces, and honestly labelled as such: no test pins it, because
  // the case it guards is unreachable today. It makes release once-per-
  // activation, so the finish()-then-teardown pair cannot clear a claim some
  // OTHER instance of the same id took in between. Today nothing can claim in
  // that window — finish() marks the id seen (blocking a same-id mount) and the
  // yield path implies tourState.active (blocking any mount) — and a deferred
  // instance never reaches releaseClaim at all, since all three call sites
  // require `active`. Keep the flag if you add a release path; it is the reason
  // an id-keyed release stays safe.
  let claimed = false;

  // Give the screen back. Idempotent, so every "active went false" path can
  // call it: finish, the yield to the main tour, and the effect teardown that
  // also covers an unmount mid-tour.
  function releaseClaim() {
    if (!claimed) return;
    claimed = false;
    if (activeTourId === id) activeTourId = null;
  }

  // While a contextual tour is on screen, flag the body so the host Modal knows
  // to route Escape to the tour instead of closing itself. The tour is
  // deliberately NON-blocking (the dim is pointer-events:none) — an earlier
  // blocking variant could trap the user behind a mispositioned popover and
  // intercepted legitimate clicks, so we only coordinate Escape here.
  // Set-and-teardown (not if/else): this effect runs on EVERY instance, and it
  // is created before onMount's effect, so an if/else `removeAttribute` branch
  // would strip another tour's flag from <body> the moment a deferred (never
  // active) instance mounts — defeating the ctx-vs-ctx mount guard below. Only
  // touch the attribute when this instance owns it.
  //
  // The teardown is load-bearing and coupled to onDestroy: it is the ONLY
  // reason `onDestroy` can safely omit `removeAttribute` (Svelte runs effect
  // teardowns on destroy too, and only for the instance whose effect ran). The
  // two edits fail independently — restore either half alone and the flag is
  // either stripped from an active tour or left stale after an unmount, so
  // change them together or not at all.
  $effect(() => {
    if (active) {
      document.body.setAttribute('data-ctx-tour-active', 'true');
      return () => {
        document.body.removeAttribute('data-ctx-tour-active');
        // Same reasoning for the module-level claim: this covers the unmount
        // path, where neither finish() nor the yield effect can run.
        releaseClaim();
      };
    }
  });

  // Yield to the main tour. Replay (Settings → Help) and a TOUR_VERSION-bump
  // re-show activate the main tour while a contextual popover can be up; two
  // live overlays freeze this one (body[data-tour-active] kills its pointer
  // events) and both window handlers answer one Escape. Deactivate WITHOUT
  // marking seen — replay just reset the flag, and the tour re-fires on the
  // next visit to its surface.
  $effect(() => {
    if (active && tourState.active) {
      active = false;
      releaseClaim();
    }
  });

  onMount(() => {
    if (hasSeen(id)) return;
    // Don't open on top of the main onboarding tour / account hint: two live
    // spotlights fight over focus and the pointer-events overlay, freezing the
    // contextual popover. Defer — the surface stays un-toured this visit and
    // re-fires next time (the "seen" flag is only set on finish).
    if (tourState.active) return;
    // Another contextual tour is on screen (cross-surface chaining: e.g. the
    // overview step's own CTA opens the translations modal, which hosts the
    // l10n tour). Same deferral as the main-tour case — this surface stays
    // un-toured this visit and re-fires on its next mount. Both sources are
    // checked: the module claim catches a sibling that activated in this same
    // flush, the attribute catches one whose effect has already painted it.
    if (activeTourId !== null || document.body.hasAttribute('data-ctx-tour-active')) return;
    activeTourId = id;
    claimed = true;
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
    // No removeAttribute here: the attribute effect's teardown handles it on
    // destroy, and only for the instance that set it — an unconditional
    // removal would strip the flag of a still-active tour when a DEFERRED
    // instance's host unmounts.
    // Host unmounted mid-tour: soft-skip so the tour doesn't re-fire on every
    // open — UNLESS the main tour's own activation tore the host down
    // (replay/startup call setMode('client') and set tourState.active in one
    // flush). Two Svelte facts dictate the shape of this check:
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
      if (active && !tourState.active) markSeen(id);
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
    releaseClaim();
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

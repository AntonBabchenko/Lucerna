<script lang="ts">
  // Always-visible (?) explainer next to the Sidebar "Instance"
  // section header. Safety net for users who skipped the tour or
  // upgraded from v0.4.x with existing instances.
  //
  // Body copy is a tightened standalone variant of the tour's
  // step-1 welcome text — deliberately NOT shared with steps.ts,
  // since the popover wants a single concise sentence while the
  // welcome step wants a brand-led intro.
  //
  // The popover is `position: fixed` (anchored to the viewport), not
  // `absolute`: the host sidebar is `overflow-y-auto`, and per the CSS
  // overflow spec that forces `overflow-x` to `auto` too — an absolute
  // popover wider than the sidebar would be clipped by it and would
  // add a horizontal scrollbar. A fixed popover escapes the sidebar's
  // overflow box. Its position is measured from the trigger each time
  // it opens and clamped into the viewport.
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { t } from '$lib/i18n';

  // Keep POPOVER_WIDTH in sync with the `w-[260px]` class on the popover.
  const POPOVER_WIDTH = 260;
  const GAP = 4;
  const MARGIN = 8;

  let open = $state(false);
  let trigger: HTMLButtonElement | undefined;
  let popoverTop = $state(0);
  let popoverLeft = $state(0);

  function positionPopover() {
    if (!trigger) return;
    const r = trigger.getBoundingClientRect();
    popoverTop = r.bottom + GAP;
    const maxLeft = window.innerWidth - POPOVER_WIDTH - MARGIN;
    popoverLeft = Math.min(Math.max(r.left, MARGIN), Math.max(MARGIN, maxLeft));
  }

  function toggle() {
    if (open) {
      open = false;
    } else {
      positionPopover();
      open = true;
    }
  }

  // A fixed popover does not follow the trigger when the layout
  // shifts, so close it on scroll/resize while it is open. `scroll`
  // is captured (third arg `true`) so it also catches the sidebar's
  // own scroll — scroll events do not bubble.
  $effect(() => {
    if (!open) return;
    const close = () => (open = false);
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    return () => {
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
    };
  });
</script>

<div class="relative inline-block">
  <button
    bind:this={trigger}
    type="button"
    class="relative z-50 text-xs text-placeholder hover:text-secondary leading-none px-1"
    aria-label={$t('onboarding.instanceConcept.triggerAriaLabel')}
    title={$t('onboarding.instanceConcept.triggerTitle')}
    aria-expanded={open}
    aria-controls="instance-concept-popover"
    onclick={toggle}
  >
    (?)
  </button>
  {#if open}
    <!-- Click-outside backdrop -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div role="presentation" class="fixed inset-0 z-30" onclick={() => (open = false)}></div>
    <div
      id="instance-concept-popover"
      class="fixed z-40 w-[260px] normal-case tracking-normal bg-surface border border-border-subtle rounded shadow-md p-2.5"
      style="top: {popoverTop}px; left: {popoverLeft}px;"
    >
      <div class="absolute top-1 right-1">
        <CloseButton
          onClick={() => (open = false)}
          ariaLabel={$t('onboarding.instanceConcept.closeAriaLabel')}
        />
      </div>
      <p class="text-xs text-secondary leading-snug pr-6">
        {$t('onboarding.instanceConcept.body')}
      </p>
    </div>
  {/if}
</div>

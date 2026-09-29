<script lang="ts">
  // A page with a contextual tour, an (i) and a dialog that can host its own
  // tour — enough to drive every layering case of ContextualTour.
  import { untrack } from 'svelte';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import type { TourStep } from '$lib/onboarding/steps';
  import HelpPopover from '$lib/ui/HelpPopover.svelte';
  import Modal from '$lib/ui/Modal.svelte';

  let {
    pageTour = true,
    nestedTour = false,
    dialogOpen = false,
  }: { pageTour?: boolean; nestedTour?: boolean; dialogOpen?: boolean } = $props();

  // Seeded synchronously (the dialog must exist before the page tour's onMount
  // in the "starts under an open dialog" cases); untrack = a deliberate
  // initial-value capture, the closure keeps svelte-check quiet about it.
  let dialog = $state(untrack(() => dialogOpen));
  let host = $state(true);

  const PAGE_STEPS: TourStep[] = [
    {
      titleKey: 'onboarding.contextual.addons.kindSwitch.title',
      bodyKey: 'onboarding.contextual.addons.kindSwitch.body',
      targetSelector: '[data-tour-ctx="fx-page"]',
      anchor: 'below',
    },
    {
      titleKey: 'onboarding.contextual.addons.subtabs.title',
      bodyKey: 'onboarding.contextual.addons.subtabs.body',
      targetSelector: '[data-tour-ctx="fx-page"]',
      anchor: 'below',
    },
  ];
  const DIALOG_STEPS: TourStep[] = [
    {
      titleKey: 'onboarding.contextual.l10n.coverage.title',
      bodyKey: 'onboarding.contextual.l10n.coverage.body',
      targetSelector: '[data-tour-ctx="fx-dialog"]',
      anchor: 'below',
    },
  ];
</script>

<div data-tour-ctx="fx-page">
  <HelpPopover
    paragraphs={['Page explanation.']}
    triggerAriaLabel="Explain page"
    closeAriaLabel="Close page explanation"
  />
  <button type="button" onclick={() => (dialog = true)}>Open dialog</button>
  <button type="button" onclick={() => (host = false)}>Leave page</button>
</div>
{#if pageTour && host}
  <ContextualTour id="addons" steps={PAGE_STEPS} />
{/if}
{#if dialog}
  <Modal ariaLabel="Fixture dialog" onClose={() => (dialog = false)}>
    <div data-tour-ctx="fx-dialog">
      <HelpPopover
        paragraphs={['Dialog explanation.']}
        triggerAriaLabel="Explain dialog"
        closeAriaLabel="Close dialog explanation"
      />
      <button type="button" onclick={() => (dialog = false)}>Close dialog</button>
    </div>
    {#if nestedTour}
      <ContextualTour id="l10n" steps={DIALOG_STEPS} />
    {/if}
  </Modal>
{/if}

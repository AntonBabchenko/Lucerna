<script lang="ts">
  // Always-visible (?) explainer next to the Sidebar "Instance" section header.
  // Safety net for users who skipped the tour or upgraded from v0.4.x with
  // existing instances. Thin wrapper over the shared HelpPopover; the copy is a
  // tightened standalone variant of the tour's step-1 welcome text (deliberately
  // not shared with steps.ts — the popover wants one concise sentence).
  import HelpPopover from '$lib/ui/HelpPopover.svelte';
  import { explanationState } from '$lib/onboarding/explanation-level.svelte';
  import { explainKey } from '$lib/onboarding/explanation-keys';
  import { t } from '$lib/i18n';
</script>

<HelpPopover
  paragraphs={[$t(explainKey('onboarding.instanceConcept.body', explanationState.level))]}
  triggerAriaLabel={$t('onboarding.instanceConcept.triggerAriaLabel')}
  triggerTitle={$t('onboarding.instanceConcept.triggerTitle')}
  closeAriaLabel={$t('onboarding.instanceConcept.closeAriaLabel')}
/>

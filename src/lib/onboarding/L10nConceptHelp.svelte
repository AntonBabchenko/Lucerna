<script lang="ts">
  // Always-visible (?) explainer beside the translations modal's title. Safety
  // net for users who skipped the l10n tour, or who never saw it at all — the
  // modal is reachable for a zero-mod instance, where the tour deliberately
  // does not fire. The tour teaches where the controls are; this answers what
  // the feature IS, and what it cannot reach (quest and config text, and the
  // opt-in AI pre-fill).
  //
  // A thin wrapper over the shared HelpPopover rather than four inline $t calls
  // in LocalizationModal: resolving one paragraph per explanation level needs a
  // TranslationKey cast, and that cast plus the derived belong in ONE place
  // even though the copy is not reused on another surface yet.
  import HelpPopover from '$lib/ui/HelpPopover.svelte';
  import { explanationState } from '$lib/onboarding/explanation-level.svelte';
  import { explainKey } from '$lib/onboarding/explanation-keys';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';

  const paras = $derived(
    ['p1', 'p2', 'p3', 'p4'].map((p) =>
      $t(explainKey(`onboarding.l10nConcept.${p}` as TranslationKey, explanationState.level)),
    ),
  );
</script>

<HelpPopover
  paragraphs={paras}
  width={320}
  triggerAriaLabel={$t('onboarding.l10nConcept.triggerAriaLabel')}
  triggerTitle={$t('onboarding.l10nConcept.triggerTitle')}
  closeAriaLabel={$t('onboarding.l10nConcept.closeAriaLabel')}
/>

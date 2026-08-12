<script lang="ts">
  // Always-visible (?) explainer for the datapack concept, shown on all three
  // datapack surfaces (the instance library, a world's Datapacks tab, and the
  // server pane) so a user meets the concept wherever they first land. Thin
  // wrapper over the shared HelpPopover — a sibling of InstanceConceptTooltip
  // and, unlike it, multi-paragraph: data packs need the "not a mod", "lives in
  // a world", and Beta caveats, not one sentence. Wrapped rather than inlined
  // because the three call sites must not drift apart in copy or width.
  import HelpPopover from '$lib/ui/HelpPopover.svelte';
  import { explanationState } from '$lib/onboarding/explanation-level.svelte';
  import { explainKey } from '$lib/onboarding/explanation-keys';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';

  // Each paragraph goes through explainKey, which swaps in the `*Basic` sibling
  // at the Basic detail level. The cast mirrors explainKey's own contract (a
  // leaf-suffix string map); a parity test pins every pN/pNBasic pair in both
  // locales, so every resolved key exists.
  const paragraphs = $derived(
    ['p1', 'p2', 'p3', 'p4'].map((p) =>
      $t(explainKey(`onboarding.datapackConcept.${p}` as TranslationKey, explanationState.level)),
    ),
  );
</script>

<HelpPopover
  {paragraphs}
  width={320}
  triggerAriaLabel={$t('onboarding.datapackConcept.triggerAriaLabel')}
  triggerTitle={$t('onboarding.datapackConcept.triggerTitle')}
  closeAriaLabel={$t('onboarding.datapackConcept.closeAriaLabel')}
/>

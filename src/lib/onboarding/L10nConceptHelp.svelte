<script lang="ts">
  // The translations concept explainer, beside the translations modal's title.
  // Safety net for users who skipped the l10n tour, or who never saw it at all —
  // the modal is reachable for a zero-mod instance, where the tour deliberately
  // does not fire. The tour teaches where the controls are; this answers what
  // the feature IS, and what it cannot reach (quest and config text, and the
  // opt-in AI pre-fill).
  //
  // Named rather than inlined in LocalizationModal so the concept has one home,
  // matching its datapack and instance siblings.
  import ConceptHelp from '$lib/onboarding/ConceptHelp.svelte';
</script>

<ConceptHelp namespace="onboarding.l10nConcept" paragraphKeys={['p1', 'p2', 'p3', 'p4']} />

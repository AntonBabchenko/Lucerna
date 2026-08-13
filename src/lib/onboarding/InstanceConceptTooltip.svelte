<script lang="ts">
  // The instance concept explainer, next to the Sidebar "Instance" section
  // header. Safety net for users who skipped the tour or upgraded from v0.4.x
  // with existing instances. The copy is a tightened standalone variant of the
  // tour's step-1 welcome text (deliberately not shared with steps.ts — this
  // wants one concise sentence, hence the single `body` leaf and the narrower
  // 260px width its multi-paragraph siblings do not use).
  //
  // Misnomer kept on purpose: this is a click-toggle popover, not a tooltip
  // (docs/DESIGN.md §8 distinguishes the two). Renaming it touches the Sidebar
  // call site and two test suites owned elsewhere, so it is left for a pass that
  // owns those files.
  import ConceptHelp from '$lib/onboarding/ConceptHelp.svelte';
</script>

<ConceptHelp namespace="onboarding.instanceConcept" paragraphKeys={['body']} width={260} />

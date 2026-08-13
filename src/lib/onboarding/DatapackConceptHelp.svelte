<script lang="ts">
  // The datapack concept explainer, shown on all three datapack surfaces (the
  // instance library, a world's Datapacks tab, and the server pane) so a user
  // meets the concept wherever they first land. Named rather than inlined three
  // times because the three surfaces must not drift apart in copy or width.
  //
  // Multi-paragraph, unlike the one-sentence instance explainer: data packs need
  // the "not a mod", "lives in a world", and Beta caveats.
  import ConceptHelp from '$lib/onboarding/ConceptHelp.svelte';
</script>

<ConceptHelp namespace="onboarding.datapackConcept" paragraphKeys={['p1', 'p2', 'p3', 'p4']} />

<script lang="ts">
  // The data pack concept explainer, shown on three surfaces so a user meets
  // the concept wherever they first land: the Add-ons kind row (through
  // AddonKindConceptHelp, for both Browse and Installed), a world's Datapacks
  // tab, and the server pane. Named rather than inlined because the surfaces
  // must not drift apart in copy or width — so every paragraph must be true on
  // all three.
  //
  // Multi-paragraph: data packs need the "not a mod", "when to pick one",
  // "lives in a world", and Beta caveats. `choose` is a named leaf rather than
  // a renumbered p3, so existing translations of p3/p4 keep their keys.
  import ConceptHelp from '$lib/onboarding/ConceptHelp.svelte';
</script>

<ConceptHelp
  namespace="onboarding.datapackConcept"
  paragraphKeys={['p1', 'p2', 'choose', 'p3', 'p4']}
/>

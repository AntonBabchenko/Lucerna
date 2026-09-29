<script lang="ts">
  // The (i) at the end of the Add-ons kind row: the concept explainer of the
  // ACTIVE content kind — what it is, what it needs, where it acts, and when
  // another kind fits better. Unlike the other named wrappers it is keyed by a
  // runtime value (the selected tab), not by one fixed concept.
  //
  // Data packs delegate to DatapackConceptHelp so this row, a world's
  // Datapacks tab and the server pane share one copy and cannot drift.
  import type { InstanceContentKind } from '$lib/mods/content-kind';
  import ConceptHelp from '$lib/onboarding/ConceptHelp.svelte';
  import DatapackConceptHelp from '$lib/onboarding/DatapackConceptHelp.svelte';

  let { kind }: { kind: InstanceContentKind } = $props();

  // One namespace per non-datapack kind. `satisfies` makes a missing entry a
  // compile error, and indexing it with a kind that has no entry fails too —
  // so a fifth content kind cannot ship without an explainer.
  const NAMESPACE = {
    mod: 'onboarding.modConcept',
    resource_pack: 'onboarding.resourcePackConcept',
    shader: 'onboarding.shaderConcept',
  } as const satisfies Record<Exclude<InstanceContentKind, 'datapack'>, string>;
</script>

{#if kind === 'datapack'}
  <DatapackConceptHelp />
{:else}
  <ConceptHelp namespace={NAMESPACE[kind]} paragraphKeys={['p1', 'p2', 'p3', 'p4']} />
{/if}

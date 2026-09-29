<script lang="ts">
  // The (i) at the end of the Add-ons kind row: the concept explainer of the
  // ACTIVE content kind — what it is, what it needs, where it acts, and when
  // another kind fits better. Unlike the other named wrappers it is keyed by a
  // runtime value (the selected tab), not by one fixed concept.
  //
  // Data packs delegate to DatapackConceptHelp so this row, a world's
  // Datapacks tab and the server pane share one copy and cannot drift.
  //
  // One LITERAL branch per kind, on purpose. A kind → namespace table read
  // back as `table[kind]` widens to the union of namespaces, and ConceptHelp
  // would then accept any leaf that exists under ANY of them — so a paragraph
  // added to one namespace only would type-check for all three and render a
  // raw key for the other two. A literal namespace per branch keeps each
  // `paragraphKeys` checked against that one namespace.
  import type { InstanceContentKind } from '$lib/mods/content-kind';
  import ConceptHelp from '$lib/onboarding/ConceptHelp.svelte';
  import DatapackConceptHelp from '$lib/onboarding/DatapackConceptHelp.svelte';

  let { kind }: { kind: InstanceContentKind } = $props();

  /** Exhaustiveness: `kind` is `never` in the final branch, so a fifth content
   *  kind fails the type check there instead of silently borrowing another
   *  kind's explainer. Unreachable at runtime; renders nothing if the type
   *  system is ever bypassed, which is honest — no explainer, no wrong one. */
  function unexplained(k: never): string {
    void k;
    return '';
  }
</script>

{#if kind === 'mod'}
  <ConceptHelp namespace="onboarding.modConcept" paragraphKeys={['p1', 'p2', 'p3', 'p4']} />
{:else if kind === 'resource_pack'}
  <ConceptHelp
    namespace="onboarding.resourcePackConcept"
    paragraphKeys={['p1', 'p2', 'p3', 'p4']}
  />
{:else if kind === 'shader'}
  <ConceptHelp namespace="onboarding.shaderConcept" paragraphKeys={['p1', 'p2', 'p3', 'p4']} />
{:else if kind === 'datapack'}
  <DatapackConceptHelp />
{:else}
  {unexplained(kind)}
{/if}

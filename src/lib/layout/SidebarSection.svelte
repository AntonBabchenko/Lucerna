<script lang="ts">
  import type { Snippet } from 'svelte';

  // Shared lower-zone sidebar section: the divider wrapper + an optional caps
  // heading. Extracted from Sidebar.svelte to dedupe the repeated
  // `flex flex-col gap-1 pt-3 border-t border-border-subtle` wrapper and the
  // `text-xs uppercase tracking-wide text-muted` caps-label recipe.
  //
  // The heading text is rendered as a DIRECT child of the caps div (never
  // wrapped in a <span>): the intent tests read `getByText(...).className`
  // directly, so the matched element must itself carry the recipe. A section
  // with no `heading` renders the divider only — the Settings footer, and the
  // Instance section whose bespoke heading is authored as the first slot child.
  //
  // Empty-group suppression stays at the CALL SITE: the caller gates the whole
  // <SidebarSection> with `{#if ... && (isVisible('a') || isVisible('b'))}`.
  let {
    heading,
    headingTestid,
    dataTour,
    children,
  }: {
    heading?: string;
    headingTestid?: string;
    dataTour?: string;
    children: Snippet;
  } = $props();
</script>

<div class="flex flex-col gap-1 pt-3 border-t border-border-subtle" data-tour={dataTour}>
  {#if heading !== undefined}
    <div class="text-xs uppercase tracking-wide text-muted" data-testid={headingTestid}>
      {heading}
    </div>
  {/if}
  {@render children()}
</div>

<script lang="ts">
  import { Icon, type IconName } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import type { Snippet } from 'svelte';
  import type { BadgeVariant } from './card-status';

  // The single status pill used by every card surface. Variant maps to the
  // semantic token pair; an optional leading icon and tooltip (via the shared
  // use:tooltip layer) are supported.
  let {
    variant = 'neutral',
    icon = undefined,
    title = undefined,
    testid = undefined,
    truncate = false,
    children,
  }: {
    variant?: BadgeVariant;
    icon?: IconName;
    title?: string | undefined;
    testid?: string | undefined;
    /** A label of any length (a modpack's name): the pill may shrink to the
        room it is given, its label cut with «…» while the icon stays whole.
        Give it a `title` that says the label in full. */
    truncate?: boolean;
    children: Snippet;
  } = $props();

  const CLASS: Record<BadgeVariant, string> = {
    success: 'bg-success-bg text-success',
    muted: 'bg-subtle text-muted',
    warning: 'bg-warning-bg text-warning-text',
    info: 'bg-accent-soft text-accent',
    neutral: 'bg-subtle text-secondary',
    danger: 'bg-danger-bg text-danger',
  };
</script>

<span
  class={`inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded ${CLASS[variant]}${truncate ? ' min-w-0 max-w-full' : ''}`}
  use:tooltip={title}
  data-testid={testid}
>
  {#if icon}<Icon name={icon} size={12} class={truncate ? 'shrink-0' : ''} />{/if}
  {#if truncate}
    <span class="min-w-0 truncate">{@render children()}</span>
  {:else}
    {@render children()}
  {/if}
</span>

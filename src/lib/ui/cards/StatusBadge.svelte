<script lang="ts">
  import { Icon, type IconName } from '$lib/ui/icons';
  import type { Snippet } from 'svelte';
  import type { BadgeVariant } from './card-status';

  // The single status pill used by every card surface. Variant maps to the
  // semantic token pair; an optional leading icon and title are supported.
  let {
    variant = 'neutral',
    icon = undefined,
    title = undefined,
    children,
  }: {
    variant?: BadgeVariant;
    icon?: IconName;
    title?: string | undefined;
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

<span class={`inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded ${CLASS[variant]}`} {title}>
  {#if icon}<Icon name={icon} size={12} />{/if}
  {@render children()}
</span>

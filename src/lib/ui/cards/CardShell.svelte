<script lang="ts">
  import type { Snippet } from 'svelte';
  import { type CardAccent, accentStripClass } from './card-status';

  // Shared outer container for every card surface. `variant` picks the form
  // (rounded tile, list row, dense compact row); `accent` paints the left strip
  // (clipped by the rounded corners on tiles via overflow-hidden); `dim` greys
  // the whole shell for disabled items. Inner layout is the caller's via the
  // default children snippet.
  let {
    variant = 'row',
    accent = 'none',
    dim = false,
    highlighted = false,
    testid = undefined,
    children,
  }: {
    variant?: 'tile' | 'row' | 'compact-row';
    accent?: CardAccent;
    dim?: boolean;
    highlighted?: boolean;
    testid?: string | undefined;
    children: Snippet;
  } = $props();

  const VARIANT: Record<'tile' | 'row' | 'compact-row', string> = {
    tile: 'relative overflow-hidden border border-border-subtle rounded-lg bg-surface p-3 flex flex-col h-full',
    row: 'relative flex items-center gap-3 pl-4 pr-3 py-2 border-b border-border-subtle bg-surface hover:bg-subtle transition-colors',
    'compact-row':
      'relative flex items-center gap-2.5 pl-4 pr-3 py-1.5 border-b border-border-subtle bg-surface hover:bg-subtle transition-colors text-sm',
  };
</script>

<div
  data-card-shell
  data-testid={testid}
  class={`${VARIANT[variant]} ${dim ? 'opacity-60' : ''} ${highlighted ? 'bg-highlight' : ''}`}
>
  <span
    data-card-accent
    aria-hidden="true"
    class={`absolute left-0 top-0 bottom-0 w-[3px] ${accentStripClass(accent)}`}
  ></span>
  {@render children()}
</div>

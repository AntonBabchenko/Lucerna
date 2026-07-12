<script lang="ts">
  /*
    Shared dialog / modal heading. Replaces the ~15 inline
    `<h3 class="font-semibold text-primary text-base|text-lg">` declarations
    that DESIGN.md flags as re-declared with varying class order.

    Forward rule (DESIGN.md §3): new dialogs default to `base`; reserve `lg`
    for full-screen-replacing flows (import / export wizards). The `text-lg`
    confirm dialogs that predate the rule pass size="lg" explicitly so their
    look is preserved during migration — they are not precedent for new ones.

    Always render the <h3> with an `id` and link it from Modal via
    `ariaLabelledby`, so the dialog has an accessible name.
  */
  import type { Snippet } from 'svelte';

  let {
    id,
    size = 'base',
    class: extraClass = '',
    children,
  }: {
    id?: string;
    size?: 'base' | 'lg';
    class?: string;
    children: Snippet;
  } = $props();
</script>

<h3 {id} class="font-semibold text-primary {size === 'lg' ? 'text-lg' : 'text-base'} {extraClass}">
  {@render children()}
</h3>

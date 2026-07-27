<script lang="ts">
  import type { Snippet } from 'svelte';
  import ContextMenu, { type ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { isVisible } from '$lib/layout/sidebar-buttons.svelte';
  import type { SidebarButtonId } from '$lib/layout/sidebar-buttons';

  // A hideable secondary sidebar button: the `isVisible(id)` gate + the
  // right-click ContextMenu hide wrapper + the shared
  // `btn-secondary btn-xs w-full flex items-center justify-center gap-1` shell.
  // Inner content (icon + label, plus any badge / status wrench) is authored by
  // the caller via `children`.
  //
  // The hide flow stays in Sidebar: `hideItems` is built there by
  // `hideMenuItems(id)` and its onSelect calls Sidebar's `requestHide(id)`. This
  // component only renders the menu wrapper — it never owns hide-confirm state.
  //
  // When `disabledTooltip` is set, the button renders disabled inside a tooltip
  // span (the data-root-unavailable import-blocked branch); otherwise it is an
  // enabled button wired to `onclick`.
  let {
    id,
    hideItems,
    contextMenuAria,
    testid,
    dataTour,
    onclick,
    disabledTooltip = null,
    children,
  }: {
    id: SidebarButtonId;
    hideItems: ContextMenuItem[];
    contextMenuAria: string;
    testid?: string;
    dataTour?: string;
    onclick?: () => void;
    disabledTooltip?: string | null;
    children: Snippet;
  } = $props();
</script>

{#if isVisible(id)}
  <ContextMenu items={hideItems} ariaLabel={contextMenuAria}>
    {#if disabledTooltip}
      <span class="inline-flex w-full" use:tooltip={{ text: disabledTooltip, describe: false }}>
        <button
          type="button"
          class="btn-secondary btn-xs w-full flex items-center justify-center gap-1"
          data-testid={testid}
          data-tour={dataTour}
          disabled
        >
          {@render children()}
        </button>
      </span>
    {:else}
      <button
        type="button"
        class="btn-secondary btn-xs w-full flex items-center justify-center gap-1"
        data-testid={testid}
        data-tour={dataTour}
        {onclick}
      >
        {@render children()}
      </button>
    {/if}
  </ContextMenu>
{/if}

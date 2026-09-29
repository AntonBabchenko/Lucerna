<script lang="ts">
  import { dragActive } from '$lib/settings/state.svelte';

  // A local-file install affordance (DESIGN.md §14, the dropzone rule). Presentational: the drop
  // itself is taken by the window-level Tauri drag-drop listeners that set `dragActive` —
  // +page.svelte (client Add-ons and Worlds, server Add-ons), ModpacksTab (the Modpacks modal) and
  // ServerImportView — never by this box, so a per-box listener cannot fight them. Those events
  // are the webview's own (enter / over / leave / drop for the whole window — no nested DOM
  // dragenter / dragleave to count): `leave` also ends a drag that left the window or was
  // cancelled, and `drop` ends one that landed.
  //  - 'full' (default): the big dashed box — an empty list, and the server-import view.
  //  - 'strip': a thin bar above a list or catalog. While a file is dragged it also paints an
  //    overlay over its HOST's content area: the nearest positioned ancestor, so every strip host
  //    wraps the strip and its content in a `relative` box. The overlay is decorative —
  //    opacity-only (§12), `pointer-events: none`, `aria-hidden` — and can never swallow a drop.
  // Either way the box itself stays the button that opens the file picker (click, Enter, Space).
  let {
    label,
    disabled = false,
    disabledLabel,
    variant = 'full',
    dragLabel,
    onClick,
  }: {
    label: string;
    disabled?: boolean;
    disabledLabel?: string;
    variant?: 'strip' | 'full';
    /** The overlay's text while a file is dragged (strip only); defaults to `label`. */
    dragLabel?: string;
    onClick: () => void;
  } = $props();

  // A disabled box takes nothing, so it lights up for nothing.
  const dragging = $derived(dragActive.value && !disabled);

  function activate() {
    if (!disabled) onClick();
  }
</script>

<div
  class="border-2 border-dashed rounded-lg text-center transition-colors {variant === 'strip'
    ? 'px-3 py-1 text-xs'
    : 'p-3 text-sm'}"
  class:cursor-pointer={!disabled}
  class:border-accent={dragging}
  class:bg-accent-soft={dragging}
  class:border-border-emphasis={!dragging}
  class:hover:border-accent={!disabled && !dragActive.value}
  class:opacity-50={disabled}
  onclick={activate}
  onkeydown={(e) => {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    // A button's Space activates it; it must not also scroll the list under it.
    e.preventDefault();
    activate();
  }}
  role="button"
  tabindex={disabled ? -1 : 0}
  aria-disabled={disabled}
  data-testid="file-dropzone"
  data-variant={variant}
>
  <span class="text-secondary">{disabled ? (disabledLabel ?? label) : label}</span>
</div>
{#if variant === 'strip'}
  <!-- z-20: above the sticky toolbars and pagers of the lists it covers (z-10). -->
  <div
    class="pointer-events-none absolute inset-0 z-20 flex justify-center rounded-lg border-2 border-dashed border-accent bg-accent-soft/90 transition-opacity duration-150"
    class:opacity-0={!dragging}
    class:opacity-100={dragging}
    aria-hidden="true"
    data-testid="file-dropzone-overlay"
  >
    <span
      class="sticky top-12 mt-12 self-start rounded-md bg-surface px-4 py-2 text-sm font-medium text-accent shadow"
      >{dragLabel ?? label}</span
    >
  </div>
{/if}

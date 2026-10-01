<script lang="ts">
  import type { DropTarget } from '$lib/layout/drop-router';
  import { dropPreview } from '$lib/settings/state.svelte';

  // A local-file install affordance (DESIGN.md §14, the dropzone rule). Presentational: the drop
  // itself is taken by the app's single window-level listener (`listenForFileDrops`), never by
  // this box, so a per-box listener cannot fight it. That listener's router names the box a drag
  // is headed for (`dropPreview.target`): only that box lights up — so the Add-ons strip stays dark
  // under the open Modpacks modal, which owns every drop while it is up, and every box stays dark
  // under a dialog that takes no files (only the topmost surface takes a drop). `target` says
  // which box this one is. The router also decides, from the dragged paths, what the drop will do
  // here: the accent look (the promise to add) only when some of the files fit, with a note on
  // what stays behind; a neutral look and the reason when none does. The events are the webview's
  // own (enter / over / leave / drop for the whole window — no nested DOM dragenter / dragleave to
  // count): `leave` also ends a drag that left the window or was cancelled, and `drop` ends one
  // that landed.
  //  - 'full' (default): the big dashed box — an empty list, and the server-import view.
  //  - 'strip': a thin bar above a list or catalog. While a file is dragged it also paints an
  //    overlay over its HOST's content area: the nearest positioned ancestor, so every strip host
  //    wraps the strip and its content in a `relative` box. The overlay is decorative —
  //    opacity-only (§12), `pointer-events: none`, `aria-hidden` — and can never swallow a drop.
  // Either way the box itself stays the button that opens the file picker (click, Enter, Space).
  let {
    target,
    label,
    disabled = false,
    disabledLabel,
    variant = 'full',
    dragLabel,
    onClick,
  }: {
    /** Which drop box this is — the router's name for it. */
    target: DropTarget;
    label: string;
    disabled?: boolean;
    disabledLabel?: string;
    variant?: 'strip' | 'full';
    /** What the box says while a drag it takes is over it — the strip's overlay, the full box's
     *  own line (plan §5c V3: the empty list kept «drag here» under the drag); defaults to `label`. */
    dragLabel?: string;
    onClick: () => void;
  } = $props();

  // What a drag headed for this box will do; null while none is — and while the box is disabled:
  // it takes nothing, so it lights up for nothing (its label already says why).
  const drag = $derived.by(() => {
    const preview = dropPreview.value;
    return preview !== null && preview.target === target && !disabled ? preview : null;
  });
  // Only a drag that adds files here gets the accent look — the promise to add. One that adds
  // nothing (`refuses`) gets the neutral look and says why instead; one that adds some says what
  // stays behind.
  const adds = $derived(drag?.adds === true);
  const refuses = $derived(drag !== null && !drag.adds);
  const notes = $derived(drag?.notes ?? []);

  function activate() {
    if (!disabled) onClick();
  }
</script>

<!-- The full box is a real target for a whole empty list (`min-h-32`), its text centred in it. -->
<div
  class="border-2 border-dashed rounded-lg text-center transition-colors {variant === 'strip'
    ? 'px-3 py-1 text-xs'
    : 'flex min-h-32 flex-col items-center justify-center gap-1 p-4 text-sm'}"
  class:cursor-pointer={!disabled}
  class:border-accent={adds}
  class:bg-accent-soft={adds}
  class:border-border-strong={!adds}
  class:hover:border-accent={!disabled && dropPreview.value === null}
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
  <!-- While files it takes are over it, the full box says what the drop will do, in the overlay's
       accent (a strip leaves that to its overlay, which covers it); a drag it takes nothing from
       leaves it its own label, and the reason goes on the note line. -->
  {#if adds && variant === 'full'}
    <span class="text-accent">{dragLabel ?? label}</span>
  {:else}
    <span class="text-secondary">{disabled ? (disabledLabel ?? label) : label}</span>
  {/if}
  <!-- The full box has no overlay: it says what a drag leaves behind itself — in a line kept even
       while there is nothing to say, so a note lands in it and the box never grows or shifts
       under the pointer (plan §5b V2). -->
  {#if variant === 'full'}
    <span class="flex min-h-4 flex-col text-xs text-secondary" data-testid="file-dropzone-notes">
      {#each notes as note}
        <span>{note}</span>
      {/each}
    </span>
  {/if}
</div>
{#if variant === 'strip'}
  <!-- z-20: above the sticky toolbars and pagers of the lists it covers (z-10). -->
  <div
    class="pointer-events-none absolute inset-0 z-20 flex justify-center rounded-lg border-2 border-dashed transition-opacity duration-150 {refuses
      ? 'border-border-strong bg-subtle/90'
      : 'border-accent bg-accent-soft/90'}"
    class:opacity-0={drag === null}
    class:opacity-100={drag !== null}
    aria-hidden="true"
    data-testid="file-dropzone-overlay"
  >
    <span
      class="sticky top-12 mt-12 flex flex-col items-center gap-1 self-start rounded-md bg-surface px-4 py-2 text-sm font-medium shadow"
    >
      {#if refuses}
        {#each notes as note}
          <span class="text-secondary">{note}</span>
        {/each}
      {:else}
        <span class="text-accent">{dragLabel ?? label}</span>
        {#each notes as note}
          <span class="text-xs font-normal text-secondary">{note}</span>
        {/each}
      {/if}
    </span>
  </div>
{/if}

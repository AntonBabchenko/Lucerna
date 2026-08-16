<script lang="ts">
  // Generic always-visible "(?)" help popover used next to section / modal
  // headers. The popover is `position: fixed` (anchored to the viewport), not
  // `absolute`: a host like the sidebar is `overflow-y-auto`, which per the CSS
  // overflow spec also forces `overflow-x: auto`, so an absolute popover wider
  // than the host would be clipped and add a horizontal scrollbar. A fixed
  // popover escapes the host's overflow box; its position is measured from the
  // trigger on open and clamped into the viewport.
  //
  // The trigger is elevated (z-50) ONLY WHILE OPEN. A persistent z-index made
  // the small "(?)" glyph paint through modals layered above its host (e.g. the
  // sidebar tooltip showing over an open dialog). While closed it stays in
  // normal flow, so any modal correctly covers it.
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import { attachPopoverDismiss } from '$lib/ui/popover-dismiss';
  import { tooltip } from '$lib/ui/tooltip';

  let {
    paragraphs,
    triggerAriaLabel,
    triggerTitle = undefined,
    closeAriaLabel,
    width = 260,
  }: {
    /**
     * The help text, one `<p>` per entry — required, so an empty popover cannot
     * be rendered by omission. A one-sentence helper passes a single-element
     * array; a concept explainer passes several. This was once a `body` /
     * `paragraphs` pair whose "provide exactly one" rule lived in a comment:
     * omitting both typechecked and rendered an empty popover, and passing both
     * silently dropped `body`. One prop makes both states unrepresentable.
     */
    paragraphs: readonly string[];
    triggerAriaLabel: string;
    /**
     * Short hover label for the (?) trigger — the terse form of
     * `triggerAriaLabel` ("Why a new instance?" vs "Why does installing a
     * modpack create a new instance?"). Routed through the shared tooltip
     * layer, never a native `title=` (docs/DESIGN.md §5). The prop name is
     * kept because it is half the `ConceptNamespace` contract in
     * `ConceptHelp.svelte` — a namespace only typechecks if it has
     * `triggerAriaLabel` + `triggerTitle` + `closeAriaLabel` leaves — and §5
     * explicitly blesses it: "A `title` prop forwarded *into* `use:tooltip`
     * internally is fine — the prop name is incidental."
     */
    triggerTitle?: string | undefined;
    closeAriaLabel: string;
    width?: number;
  } = $props();

  const GAP = 4;
  const MARGIN = 8;
  const popoverId = `help-popover-${crypto.randomUUID()}`;

  let open = $state(false);
  let trigger: HTMLButtonElement | undefined;
  let popoverTop = $state(0);
  let popoverLeft = $state(0);

  function positionPopover() {
    if (!trigger) return;
    const r = trigger.getBoundingClientRect();
    popoverTop = r.bottom + GAP;
    const maxLeft = window.innerWidth - width - MARGIN;
    popoverLeft = Math.min(Math.max(r.left, MARGIN), Math.max(MARGIN, maxLeft));
  }

  function toggle() {
    if (open) {
      open = false;
    } else {
      positionPopover();
      open = true;
    }
  }

  // A fixed popover does not follow the trigger when the layout shifts, so close
  // it on scroll/resize while open (shared helper). Escape dismisses it too, but
  // it also refocuses the trigger (keyboard dismiss), so it stays a bespoke
  // handler rather than routing through the helper.
  $effect(() => {
    if (!open) return;
    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        open = false;
        trigger?.focus();
      }
    };
    window.addEventListener('keydown', onKeydown);
    const detach = attachPopoverDismiss({ onDismiss: () => (open = false) });
    return () => {
      window.removeEventListener('keydown', onKeydown);
      detach();
    };
  });
</script>

<div class="relative inline-block">
  <button
    bind:this={trigger}
    type="button"
    class="relative text-xs text-placeholder hover:text-secondary leading-none px-1"
    class:z-50={open}
    aria-label={triggerAriaLabel}
    use:tooltip={triggerTitle}
    aria-expanded={open}
    aria-controls={popoverId}
    onclick={toggle}
  >
    <Icon name="info" size={14} />
  </button>
  {#if open}
    <!-- Click-outside backdrop -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div role="presentation" class="fixed inset-0 z-30" onclick={() => (open = false)}></div>
    <div
      id={popoverId}
      class="fixed z-[var(--z-popover)] normal-case tracking-normal bg-surface border border-border-subtle rounded shadow-md p-2.5"
      style="top: {popoverTop}px; left: {popoverLeft}px; width: {width}px;"
    >
      <div class="absolute top-1 right-1">
        <CloseButton onClick={() => (open = false)} ariaLabel={closeAriaLabel} />
      </div>
      <div class="space-y-2">
        <!-- Index-keyed: paragraphs are ordered static text, and a text key
             throws Svelte's each_key_duplicate when two paragraphs match.
             `pr-6` sits on each <p> rather than on this wrapper so the single-
             paragraph case renders byte-identically to the old `body` branch —
             the close button's clearance is a property of the text, not of the
             stack. -->
        {#each paragraphs as para, i (i)}
          <p class="text-xs text-secondary leading-snug pr-6">{para}</p>
        {/each}
      </div>
    </div>
  {/if}
</div>

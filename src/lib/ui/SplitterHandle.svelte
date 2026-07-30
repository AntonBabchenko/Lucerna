<script lang="ts">
  import { clampPanelWidth, SPLITTER_KEY_STEP } from '$lib/ui/splitter';

  let {
    width = $bindable(),
    min,
    max,
    label,
    side = 'start',
    testId,
  }: {
    /** Width (px) of the pane this handle resizes. */
    width: number;
    min: number;
    max: number;
    /** Accessible name — describes what is being resized. */
    label: string;
    /** Which side of the handle the resized pane sits on. `start` (the
     *  default) means it is before the handle, so dragging right widens it;
     *  `end` means it is after, so dragging left widens it. */
    side?: 'start' | 'end';
    testId?: string;
  } = $props();

  // +1 when dragging right should widen the pane, -1 when it should narrow it.
  const dir = $derived(side === 'start' ? 1 : -1);

  function onPointerDown(e: PointerEvent): void {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startWidth = width;
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const onMove = (ev: PointerEvent): void => {
      width = clampPanelWidth(startWidth + dir * (ev.clientX - startX), min, max);
    };
    const onUp = (ev: PointerEvent): void => {
      if (handle.hasPointerCapture(ev.pointerId)) handle.releasePointerCapture(ev.pointerId);
      handle.removeEventListener('pointermove', onMove);
      handle.removeEventListener('pointerup', onUp);
      handle.removeEventListener('pointercancel', onUp);
    };
    handle.addEventListener('pointermove', onMove);
    handle.addEventListener('pointerup', onUp);
    // OS-cancelled pointer (e.g. a system gesture takes over): still clean up.
    handle.addEventListener('pointercancel', onUp);
  }

  function onKeyDown(e: KeyboardEvent): void {
    if (e.key === 'ArrowRight') width = clampPanelWidth(width + dir * SPLITTER_KEY_STEP, min, max);
    else if (e.key === 'ArrowLeft')
      width = clampPanelWidth(width - dir * SPLITTER_KEY_STEP, min, max);
    else return;
    e.preventDefault();
  }
</script>

<!-- A focusable window splitter is a valid ARIA pattern that the a11y linter
     flags as non-interactive. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={width}
  aria-valuemin={min}
  aria-valuemax={max}
  tabindex={0}
  data-testid={testId}
  class="w-1 shrink-0 cursor-col-resize bg-border-subtle hover:bg-border-emphasis focus-visible:bg-accent focus:outline-none"
  onpointerdown={onPointerDown}
  onkeydown={onKeyDown}
></div>

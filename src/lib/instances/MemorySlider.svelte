<script lang="ts">
  import type { MemoryBounds } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { formatHeapLabel, isAboveRecommended } from '$lib/instances/heap';
  import { FALLBACK_MEMORY_BOUNDS, loadMemoryBounds } from '$lib/instances/memory-bounds';

  let {
    valueMb,
    onInput,
    onCommit,
    id,
    class: extraClass = '',
    warnClass = '',
    reserveWarnSpace = false,
  }: {
    /** Current heap in MB. The parent owns and persists this value. */
    valueMb: number;
    /** Fired with the parsed MB on every drag tick — for live UI (no persist). */
    onInput: (mb: number) => void;
    /**
     * Optional: fired once with the parsed MB when the drag is released
     * (the input's `change` event). Auto-saving surfaces persist here so they
     * write once per drag instead of on every tick. Surfaces that persist on a
     * separate Save/Create action simply omit it.
     */
    onCommit?: (mb: number) => void;
    /** Optional id so an external `<label for=…>` can target the range input. */
    id?: string;
    /** Extra classes appended to the range input (base is always `w-full`). */
    class?: string;
    /** Extra classes on the high-memory warning paragraph. */
    warnClass?: string;
    /** Render an empty spacer when no warning shows, to avoid layout shift. */
    reserveWarnSpace?: boolean;
  } = $props();

  // Adaptive bounds for this machine, fetched once. Until they resolve we use
  // the static fallback so the control is usable immediately.
  let bounds = $state<MemoryBounds>(FALLBACK_MEMORY_BOUNDS);
  $effect(() => {
    let alive = true;
    void loadMemoryBounds().then((b) => {
      if (alive) bounds = b;
    });
    return () => {
      alive = false;
    };
  });

  // Keep the rendered thumb in sync with `valueMb`. A range input clamps its DOM
  // value to the current `max`; when we start at the 8 GB fallback ceiling and
  // the configured heap is larger (e.g. 19 GB), the browser pins the thumb to
  // 8 GB and never restores it once `max` grows to real RAM — the one-way
  // `value` binding doesn't re-fire because the state is unchanged. Re-applying
  // the value in an effect that runs AFTER the `max` attribute update fixes the
  // thumb without mutating the parent's state.
  let el = $state<HTMLInputElement>();
  $effect(() => {
    void bounds.max_mb; // re-run when the ceiling changes
    if (el) el.value = String(valueMb);
  });

  function handleInput(event: Event) {
    onInput(parseInt((event.currentTarget as HTMLInputElement).value, 10));
  }

  function handleChange(event: Event) {
    onCommit?.(parseInt((event.currentTarget as HTMLInputElement).value, 10));
  }
</script>

<input
  {id}
  bind:this={el}
  type="range"
  min={bounds.min_mb}
  max={bounds.max_mb}
  step={bounds.step_mb}
  value={valueMb}
  aria-valuetext={formatHeapLabel(valueMb)}
  oninput={handleInput}
  onchange={handleChange}
  class="w-full {extraClass}"
/>
{#if isAboveRecommended(valueMb, bounds.recommended_max_mb, bounds.ram_known)}
  <p class="text-xs text-warning-text {warnClass}">
    {$t('instance.manage.memoryWarnHigh', {
      recommended: formatHeapLabel(bounds.recommended_max_mb),
    })}
  </p>
{:else if reserveWarnSpace}
  <div class="mb-3"></div>
{/if}

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

  // Round to the nearest step and clamp to [min, max] — mirrors the backend's
  // round_to_step so a typed value can never persist outside the valid range.
  function clampRound(raw: number): number {
    const stepped = Math.round(raw / bounds.step_mb) * bounds.step_mb;
    return Math.min(bounds.max_mb, Math.max(bounds.min_mb, stepped));
  }

  // The numeric field mirrors the range: live (unclamped) on input so typing
  // isn't fought, clamped+rounded+committed on change (blur/Enter).
  //
  // We deliberately do NOT clamp on input. The field is controlled
  // (value={valueMb}), so feeding a clamped value to onInput would round-trip
  // back through valueMb and reset the DOM field mid-keypress — the cursor would
  // jump and a partial entry like "16" would snap to the min. The trade-off is a
  // transient out-of-range live preview when someone types an absurd value; it
  // self-corrects on the change event (clampRound) and nothing persists until
  // onCommit fires there.
  function handleNumberInput(event: Event) {
    const raw = parseInt((event.currentTarget as HTMLInputElement).value, 10);
    if (Number.isNaN(raw)) return; // mid-edit / empty — wait for the change event
    onInput(raw);
  }

  function handleNumberChange(event: Event) {
    const field = event.currentTarget as HTMLInputElement;
    const raw = parseInt(field.value, 10);
    const v = Number.isNaN(raw) ? valueMb : clampRound(raw);
    onInput(v); // sync the live value to the corrected one
    onCommit?.(v);
    field.value = String(v); // reflect the clamp/round back into the field
  }

  // Recommended-max tick: only meaningful when RAM is known and the marker sits
  // strictly inside the track.
  const showRecommended = $derived(
    bounds.ram_known &&
      bounds.recommended_max_mb > bounds.min_mb &&
      bounds.recommended_max_mb < bounds.max_mb,
  );
  const recommendedPct = $derived(
    bounds.max_mb > bounds.min_mb
      ? ((bounds.recommended_max_mb - bounds.min_mb) / (bounds.max_mb - bounds.min_mb)) * 100
      : 0,
  );

  const numberId = $derived(id ? `${id}-num` : undefined);
</script>

<div class="flex items-center gap-2">
  <div class="relative flex-1">
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
    {#if showRecommended}
      <span
        class="pointer-events-none absolute top-1/2 h-2.5 w-px -translate-y-1/2 bg-warning-text/60"
        style="left: {recommendedPct}%"
        aria-hidden="true"
      ></span>
    {/if}
    <div class="mt-0.5 flex justify-between text-[10px] leading-none text-placeholder">
      <span>{formatHeapLabel(bounds.min_mb)}</span>
      <span>{formatHeapLabel(bounds.max_mb)}</span>
    </div>
  </div>
  <input
    type="number"
    id={numberId}
    aria-label={$t('instance.manage.memoryInputLabel')}
    min={bounds.min_mb}
    max={bounds.max_mb}
    step={bounds.step_mb}
    value={valueMb}
    oninput={handleNumberInput}
    onchange={handleNumberChange}
    class="w-20 shrink-0 rounded border px-2 py-1 text-sm"
  />
</div>
{#if showRecommended}
  <span class="sr-only"
    >{$t('instance.manage.memoryRecommendedMarker', {
      value: formatHeapLabel(bounds.recommended_max_mb),
    })}</span
  >
{/if}
{#if isAboveRecommended(valueMb, bounds.recommended_max_mb, bounds.ram_known)}
  <p class="text-xs text-warning-text {warnClass}">
    {$t('instance.manage.memoryWarnHigh', {
      recommended: formatHeapLabel(bounds.recommended_max_mb),
    })}
  </p>
{:else if reserveWarnSpace}
  <div class="mb-3"></div>
{/if}

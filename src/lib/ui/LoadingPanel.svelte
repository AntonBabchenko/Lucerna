<script lang="ts">
  // Full content-area loading state: a centered spinner with a visible label
  // beneath it. Use for panels / dialog bodies that are otherwise empty while
  // content loads. Inline / per-row loads should use
  // <Spinner labelPlacement="right" .../> directly instead.
  //
  // `detail` is an optional muted line under the label for work that can say
  // how far along it is («37 of 136»). Pass `null` until there is something to
  // say: the line's live region is rendered whenever the prop is given at all,
  // so it exists before its first update — a region created WITH its text is
  // often not announced. Leave the prop out and nothing extra is rendered.
  import Spinner from '$lib/ui/Spinner.svelte';

  interface Props {
    label: string;
    size?: 'sm' | 'md' | 'lg';
    delayMs?: number;
    detail?: string | null;
  }
  let { label, size = 'lg', delayMs = 150, detail }: Props = $props();
</script>

<div class="flex flex-col items-center justify-center gap-2 py-8 text-secondary">
  <Spinner {size} {delayMs} labelPlacement="below" {label} />
  {#if detail !== undefined}
    <p class="text-xs text-muted" aria-live="polite" data-testid="loading-panel-detail">
      {detail ?? ''}
    </p>
  {/if}
</div>

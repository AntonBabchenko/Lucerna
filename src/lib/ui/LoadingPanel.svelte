<script lang="ts">
  // Full content-area loading state: a centered spinner with a visible label
  // beneath it. Use for panels / dialog bodies that are otherwise empty while
  // content loads. Inline / per-row loads should use
  // <Spinner labelPlacement="right" .../> directly instead.
  import Spinner from '$lib/ui/Spinner.svelte';

  interface Props {
    label: string;
    size?: 'sm' | 'md' | 'lg';
    delayMs?: number;
  }
  let { label, size = 'lg', delayMs = 150 }: Props = $props();
</script>

<div class="flex flex-col items-center justify-center gap-2 py-8 text-secondary">
  <Spinner {size} {delayMs} labelPlacement="below" {label} />
</div>

<script lang="ts">
  // Test fixture: a list host wired to `rereadOnReturn`, with its three conditions as props the test
  // flips. testing-library's rerender replaces the whole props object, so every prop read re-runs —
  // harmless here, and the reason the views' own tests do not count reads across a rerender.
  import { rereadOnReturn } from '$lib/ui/reread-on-return.svelte';

  let {
    active = true,
    visible = true,
    idle = true,
    onReread,
  }: { active?: boolean; visible?: boolean; idle?: boolean; onReread: () => void } = $props();

  rereadOnReturn({
    active: () => active,
    visible: () => visible,
    idle: () => idle,
    reread: () => onReread(),
  });
</script>

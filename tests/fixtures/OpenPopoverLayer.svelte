<script lang="ts">
  // Test fixture: an open popover, registered the way every popover is — `useLayer`, whose
  // `$effect` pushes the layer. Lets a test see whether that effect ever re-runs (it would
  // release and re-push the layer, which puts it back on top).
  import { useLayer } from '$lib/ui/layer-stack.svelte';

  useLayer(
    'popover',
    () => true,
    () => {},
  );
</script>

<script lang="ts">
  import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
  import type { LoaderKind } from '$lib/ipc/bindings';

  // Mirrors the create form: `bind:` and NO onchange. The bound pair is
  // rendered as text so a test can read what the parent actually holds.
  let loader = $state<LoaderKind>('fabric');
  let loaderVersion = $state<string | null>('0.16.0');
</script>

<LoaderPicker mc="1.20.1" bind:loader bind:loaderVersion />
<output data-testid="bound">{loader}|{loaderVersion ?? 'null'}</output>

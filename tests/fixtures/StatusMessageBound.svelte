<script lang="ts">
  // Binds StatusMessage's region so a test can read what a caller would get.
  import StatusMessage from '$lib/ui/StatusMessage.svelte';

  let { message = null }: { message?: string | null } = $props();
  let element = $state<HTMLElement | null>(null);
</script>

<StatusMessage bind:element {message} tone="danger" />
<output data-testid="bound">{element?.getAttribute('role') ?? 'none'}</output>

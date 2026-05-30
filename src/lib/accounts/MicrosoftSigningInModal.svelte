<script lang="ts">
  let {
    open = false,
    onCancel,
  }: {
    open?: boolean;
    onCancel?: () => void;
  } = $props();

  function handleEscape(e: KeyboardEvent) {
    if (e.key === 'Escape' && open) {
      onCancel?.();
    }
  }
</script>

<svelte:window onkeydown={handleEscape} />

{#if open}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
    <div class="bg-surface rounded-lg shadow-xl max-w-md w-full mx-4 p-6">
      <h3 class="text-lg font-semibold text-primary">Complete sign-in in your browser</h3>
      <p class="mt-3 text-sm text-secondary">
        A Microsoft sign-in tab should have opened. Finish there, then return to this window.
        Waiting up to 5 minutes.
      </p>
      <div class="mt-6 flex justify-end gap-2">
        <button type="button" class="btn-secondary btn-sm" onclick={() => onCancel?.()}>
          Cancel
        </button>
      </div>
    </div>
  </div>
{/if}

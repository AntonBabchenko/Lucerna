<script lang="ts">
  import { t } from '$lib/i18n';

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
      <h3 class="text-lg font-semibold text-primary">{$t('accounts.signingIn.heading')}</h3>
      <p class="mt-3 text-sm text-secondary">
        {$t('accounts.signingIn.body')}
      </p>
      <div class="mt-6 flex justify-end gap-2">
        <button type="button" class="btn-secondary btn-sm" onclick={() => onCancel?.()}>
          {$t('common.cancel')}
        </button>
      </div>
    </div>
  </div>
{/if}

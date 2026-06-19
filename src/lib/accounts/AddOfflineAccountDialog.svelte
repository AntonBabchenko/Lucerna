<script lang="ts">
  // Add-an-offline-account dialog. Replaces an inline name field that lived
  // tucked under the sidebar "Add offline" button: users clicked the button,
  // missed the quietly-revealed input (no autofocus, placeholder-only, blended
  // into the narrow sidebar), and reported that it wasn't clear anything had to
  // be typed at all. A centred modal with a heading, a labelled autofocused
  // input, and an explicit primary action makes the "now enter a name" step
  // impossible to miss. Purely presentational: the actual addOfflineAccount
  // call lives in the page handler, which keeps this open on error.
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';

  let {
    error,
    onCancel,
    onSubmit,
  }: {
    /** Backend error from the last submit, or null. Rendered inside the modal. */
    error: string | null;
    onCancel: () => void;
    onSubmit: (name: string) => void;
  } = $props();

  let name = $state('');
  const trimmed = $derived(name.trim());

  function submit() {
    if (trimmed.length === 0) return;
    onSubmit(trimmed);
  }
</script>

<Modal
  ariaLabelledby="add-offline-title"
  onClose={onCancel}
  panelClass="w-[440px] p-5 flex flex-col gap-3"
>
  <h3 id="add-offline-title" class="font-semibold text-primary text-base">
    {$t('page.accounts.addOfflineTitle')}
  </h3>
  <p class="text-sm text-secondary">
    {$t('page.accounts.addOfflineDescription')}
  </p>

  <form
    class="flex flex-col gap-3"
    onsubmit={(e) => {
      e.preventDefault();
      submit();
    }}
  >
    <div class="flex flex-col gap-1">
      <label class="text-xs text-secondary" for="add-offline-name">
        {$t('page.accounts.addOfflineNameLabel')}
      </label>
      <input
        id="add-offline-name"
        data-autofocus
        class="border rounded px-2 py-1 text-sm w-full"
        bind:value={name}
        maxlength="16"
        autocomplete="off"
        spellcheck="false"
        placeholder={$t('sidebar.playerNamePlaceholder')}
      />
    </div>

    {#if error}
      <p class="text-sm text-danger" role="alert">{error}</p>
    {/if}

    <div class="flex justify-end gap-2 mt-2">
      <button type="button" class="btn-secondary btn-sm" onclick={onCancel}>
        {$t('common.cancel')}
      </button>
      <button type="submit" class="btn-primary btn-sm" disabled={trimmed.length === 0}>
        {$t('page.accounts.addOfflineConfirm')}
      </button>
    </div>
  </form>
</Modal>

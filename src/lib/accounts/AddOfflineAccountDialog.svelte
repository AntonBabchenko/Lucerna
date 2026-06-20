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
  import { offlineNameRejectionKey, validateOfflineName } from '$lib/accounts/offline-name';

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
  // null when valid. Surface the reason only once the user has typed something,
  // so an empty field shows the neutral helper, not a premature "too short".
  const rejection = $derived(trimmed.length > 0 ? validateOfflineName(trimmed) : null);
  const canSubmit = $derived(trimmed.length > 0 && rejection === null);

  function submit() {
    if (!canSubmit) return;
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
      <!-- maxlength clamps typed input to 16, so the `too_long` reason hint
           isn't reachable by typing here; validateOfflineName still covers
           too_long for names reaching us via other routes (sidebar flag on a
           stored name, the backend OfflineNameInvalid error). -->
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
      <p class="text-xs text-muted">{$t('page.accounts.offlineNameHelp')}</p>
      {#if rejection}
        <p class="text-xs text-danger" role="status">
          {$t(offlineNameRejectionKey(rejection))}
        </p>
      {/if}
    </div>

    {#if error}
      <p class="text-sm text-danger" role="alert">{error}</p>
    {/if}

    <div class="flex justify-end gap-2 mt-2">
      <button type="button" class="btn-secondary btn-sm" onclick={onCancel}>
        {$t('common.cancel')}
      </button>
      <button type="submit" class="btn-primary btn-sm" disabled={!canSubmit}>
        {$t('page.accounts.addOfflineConfirm')}
      </button>
    </div>
  </form>
</Modal>

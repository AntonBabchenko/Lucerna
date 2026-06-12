<script lang="ts">
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { isValidServerAddress } from '$lib/worlds/quick-join';

  let {
    open,
    busy = false,
    showOfflineHint = false,
    onJoin,
    onClose,
  }: {
    open: boolean;
    busy?: boolean;
    showOfflineHint?: boolean;
    onJoin: (address: string) => void;
    onClose: () => void;
  } = $props();

  let address = $state('');
  let touched = $state(false);

  const trimmed = $derived(address.trim());
  const valid = $derived(isValidServerAddress(trimmed));

  // Reset state each time the dialog opens. The parent mounts this component
  // persistently and toggles `open`, so the component is not remounted between
  // opens — without this, a previously-typed address/error would linger.
  $effect(() => {
    if (open) {
      address = '';
      touched = false;
    }
  });

  function submit() {
    touched = true;
    if (!valid) return;
    onJoin(trimmed);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      submit();
    }
  }
</script>

{#if open}
  <Modal
    ariaLabelledby="quick-join-title"
    {onClose}
    panelClass="max-w-md w-full p-4"
    closeOnBackdrop={!busy}
    closeOnEscape={!busy}
  >
    <h3 id="quick-join-title" class="font-semibold text-lg text-primary mb-3">
      {$t('quickJoin.title')}
    </h3>

    <label class="block text-xs text-secondary mb-1" for="quick-join-address">
      {$t('quickJoin.addressLabel')}
    </label>
    <input
      id="quick-join-address"
      class="border rounded px-2 py-1 w-full mb-1"
      bind:value={address}
      disabled={busy}
      placeholder={$t('quickJoin.addressPlaceholder')}
      autocomplete="off"
      aria-invalid={touched && !valid}
      aria-describedby="quick-join-error"
      onkeydown={onKeydown}
      onblur={() => (touched = true)}
    />

    <p id="quick-join-error" class="text-xs text-danger mb-2 min-h-4" role="alert">
      {#if touched && !valid}{$t('quickJoin.invalidAddress')}{/if}
    </p>

    {#if showOfflineHint}
      <p class="text-xs text-secondary mb-3">{$t('quickJoin.offlineHint')}</p>
    {/if}

    <div class="flex justify-end gap-2">
      <button type="button" class="btn-secondary btn-sm" onclick={onClose} disabled={busy}>
        {$t('quickJoin.cancel')}
      </button>
      <BusyButton class="btn-primary btn-sm" {busy} disabled={touched && !valid} onclick={submit}>
        {$t('quickJoin.join')}
      </BusyButton>
    </div>
  </Modal>
{/if}

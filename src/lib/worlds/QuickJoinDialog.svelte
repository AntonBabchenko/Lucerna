<script lang="ts">
  import { untrack } from 'svelte';
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Icon from '$lib/ui/icons/Icon.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { isValidServerAddress } from '$lib/worlds/quick-join';
  import type { SavedServer } from '$lib/ipc/bindings';

  let {
    open,
    savedServers = [],
    busy = false,
    connectDisabledReason = null,
    addDisabledReason = null,
    showOfflineHint = false,
    onConnect,
    onSave,
    onSaveAndConnect,
    onDelete,
    onClose,
  }: {
    open: boolean;
    savedServers?: SavedServer[];
    busy?: boolean;
    connectDisabledReason?: string | null;
    addDisabledReason?: string | null;
    showOfflineHint?: boolean;
    onConnect: (address: string) => void;
    onSave: (name: string, address: string) => void;
    onSaveAndConnect: (name: string, address: string) => void;
    onDelete: (index: number, address: string) => void;
    onClose: () => void;
  } = $props();

  let name = $state('');
  let address = $state('');
  let touched = $state(false);
  let addOpen = $state(false);
  // Index awaiting delete confirmation, or null.
  let confirmingIndex = $state<number | null>(null);

  const trimmedAddress = $derived(address.trim());
  const trimmedName = $derived(name.trim());
  const addressValid = $derived(isValidServerAddress(trimmedAddress));
  const canSave = $derived(addressValid && trimmedName.length > 0 && addDisabledReason === null);

  // Reset transient state on each open; auto-expand the add section when there
  // are no saved servers (otherwise the dialog would offer no obvious action).
  $effect(() => {
    if (open) {
      name = '';
      address = '';
      touched = false;
      confirmingIndex = null;
      addOpen = untrack(() => savedServers.length === 0);
    }
  });

  function submitSave(connectAfter: boolean) {
    touched = true;
    if (!canSave) return;
    if (connectAfter) onSaveAndConnect(trimmedName, trimmedAddress);
    else onSave(trimmedName, trimmedAddress);
  }
</script>

{#if open}
  <Modal
    ariaLabelledby="servers-title"
    {onClose}
    panelClass="max-w-md w-full p-4"
    closeOnBackdrop={!busy}
    closeOnEscape={!busy}
  >
    <h3 id="servers-title" class="font-semibold text-lg text-primary mb-3 flex items-center gap-2">
      <Icon name="globe" size={18} />
      {$t('quickJoin.title')}
    </h3>

    {#if savedServers.length > 0}
      <p class="text-xs text-secondary mb-2">{$t('quickJoin.savedHeading')}</p>
      <ul class="flex flex-col gap-2 mb-3">
        {#each savedServers as server, i (i)}
          <li class="flex items-center gap-2 bg-surface-subtle rounded px-3 py-2">
            {#if confirmingIndex === i}
              <span class="flex-1 text-xs text-danger min-w-0">
                {$t('quickJoin.deleteConfirm', { name: server.name })}
              </span>
              <button
                type="button"
                class="btn-secondary btn-sm"
                disabled={busy}
                onclick={() => (confirmingIndex = null)}
              >
                {$t('quickJoin.deleteCancel')}
              </button>
              <button
                type="button"
                class="btn-danger btn-sm"
                disabled={busy}
                onclick={() => onDelete(i, server.address)}
              >
                {$t('quickJoin.deleteConfirmAction')}
              </button>
            {:else}
              <div class="flex-1 min-w-0">
                <p class="text-sm font-medium text-primary truncate">{server.name}</p>
                <p class="text-xs text-secondary truncate">{server.address}</p>
              </div>
              <button
                type="button"
                class="btn-success btn-sm flex items-center gap-1.5"
                disabled={busy || connectDisabledReason !== null}
                use:tooltip={connectDisabledReason ?? undefined}
                onclick={() => onConnect(server.address)}
              >
                <Icon name="play" size={14} />
                {$t('quickJoin.connect')}
              </button>
              <button
                type="button"
                class="btn-icon"
                aria-label={$t('quickJoin.delete')}
                disabled={busy}
                onclick={() => (confirmingIndex = i)}
              >
                <Icon name="trash" size={15} />
              </button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    <details class="border-t border-border-subtle pt-3" bind:open={addOpen}>
      <summary class="flex items-center gap-2 cursor-pointer text-sm font-medium text-primary">
        <Icon name={addOpen ? 'chevronDown' : 'caret'} size={16} />
        {$t('quickJoin.addSection')}
      </summary>

      <div class="mt-3">
        <label class="block text-xs text-secondary mb-1" for="server-name">
          {$t('quickJoin.nameLabel')}
        </label>
        <input
          id="server-name"
          class="border rounded px-2 py-1 w-full mb-1"
          bind:value={name}
          disabled={busy}
          placeholder={$t('quickJoin.namePlaceholder')}
          autocomplete="off"
          aria-invalid={touched && trimmedName.length === 0}
        />
        <p class="text-xs text-danger mb-2 min-h-4" role="alert">
          {#if touched && trimmedName.length === 0}{$t('quickJoin.invalidName')}{/if}
        </p>

        <label class="block text-xs text-secondary mb-1" for="server-address">
          {$t('quickJoin.addressLabel')}
        </label>
        <input
          id="server-address"
          class="border rounded px-2 py-1 w-full mb-1"
          bind:value={address}
          disabled={busy}
          placeholder={$t('quickJoin.addressPlaceholder')}
          autocomplete="off"
          aria-invalid={touched && !addressValid}
        />
        <p class="text-xs text-danger mb-2 min-h-4" role="alert">
          {#if touched && !addressValid}{$t('quickJoin.invalidAddress')}{/if}
        </p>

        {#if showOfflineHint}
          <p class="text-xs text-secondary mb-2">{$t('quickJoin.offlineHint')}</p>
        {/if}

        <div class="flex flex-col gap-2 sm:flex-row sm:justify-end">
          <span use:tooltip={addDisabledReason ?? undefined} class="w-full sm:w-auto">
            <BusyButton
              class="btn-secondary btn-sm w-full sm:w-auto"
              {busy}
              disabled={!canSave}
              onclick={() => submitSave(false)}
            >
              {$t('quickJoin.save')}
            </BusyButton>
          </span>
          <span
            use:tooltip={addDisabledReason ?? connectDisabledReason ?? undefined}
            class="w-full sm:w-auto"
          >
            <BusyButton
              class="btn-primary btn-sm w-full sm:w-auto"
              {busy}
              disabled={!canSave || connectDisabledReason !== null}
              onclick={() => submitSave(true)}
            >
              {$t('quickJoin.saveAndConnect')}
            </BusyButton>
          </span>
        </div>
      </div>
    </details>

    <div class="flex justify-end mt-3">
      <button type="button" class="btn-secondary btn-sm" onclick={onClose} disabled={busy}>
        {$t('quickJoin.cancel')}
      </button>
    </div>
  </Modal>
{/if}

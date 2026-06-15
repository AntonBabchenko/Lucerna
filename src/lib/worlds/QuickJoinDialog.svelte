<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
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
    // Resolve `true` on a successful save so the dialog can clear + collapse.
    onSave: (name: string, address: string) => Promise<boolean>;
    onSaveAndConnect: (name: string, address: string) => Promise<boolean>;
    onDelete: (index: number, address: string) => void;
    onClose: () => void;
  } = $props();

  let name = $state('');
  let address = $state('');
  let touched = $state(false);
  let addOpen = $state(false);
  // Index awaiting delete confirmation, or null.
  let confirmingIndex = $state<number | null>(null);
  // Index whose address was just copied (drives the transient ✓ feedback), or null.
  let copiedIndex = $state<number | null>(null);

  const COPIED_FEEDBACK_MS = 1200;
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  function copyAddress(index: number, address: string) {
    void navigator.clipboard.writeText(address);
    copiedIndex = index;
    clearTimeout(copyTimer);
    copyTimer = setTimeout(() => {
      copiedIndex = null;
    }, COPIED_FEEDBACK_MS);
  }

  onDestroy(() => clearTimeout(copyTimer));

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
      copiedIndex = null;
      addOpen = untrack(() => savedServers.length === 0);
    }
  });

  async function submitSave(connectAfter: boolean) {
    touched = true;
    if (!canSave) return;
    const ok = connectAfter
      ? await onSaveAndConnect(trimmedName, trimmedAddress)
      : await onSave(trimmedName, trimmedAddress);
    // On success, clear the form and collapse the add section so the next
    // open / next add starts fresh and the saved list reads as the primary view.
    if (ok) {
      name = '';
      address = '';
      touched = false;
      addOpen = false;
    }
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
      <!-- Cap height + scroll so a long list stays inside the modal instead of
           overflowing the viewport. -->
      <ul class="flex flex-col gap-2 mb-3 max-h-72 overflow-y-auto pr-1">
        {#each savedServers as server, i (i)}
          <li
            class="flex items-center gap-2 bg-surface-subtle border border-border-subtle rounded px-3 py-2"
          >
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
              <!-- Click the name/address block to copy the address; ✓ + tooltip
                   give transient feedback. Keeps the row free of an extra button. -->
              <button
                type="button"
                class="flex-1 min-w-0 text-left cursor-pointer rounded transition-opacity hover:opacity-80"
                aria-label={$t('quickJoin.copyAddress')}
                use:tooltip={$t(copiedIndex === i ? 'quickJoin.copied' : 'quickJoin.copyAddress')}
                onclick={() => copyAddress(i, server.address)}
              >
                <p class="text-sm font-medium text-primary truncate">{server.name}</p>
                <p class="text-xs text-secondary truncate flex items-center gap-1">
                  <span class="truncate">{server.address}</span>
                  {#if copiedIndex === i}<Icon name="success" size={12} />{/if}
                </p>
              </button>
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
    {:else}
      <p class="text-xs text-secondary mb-3">{$t('quickJoin.empty')}</p>
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

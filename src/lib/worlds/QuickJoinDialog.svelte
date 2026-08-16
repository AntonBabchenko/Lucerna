<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Icon from '$lib/ui/icons/Icon.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { isValidServerAddress } from '$lib/worlds/quick-join';
  import { formatPingChip, type PingState } from '$lib/worlds/server-ping';
  import type { SavedServer } from '$lib/ipc/bindings';

  let {
    open,
    savedServers = [],
    savedServersLoading = false,
    savedServersError = null,
    busy = false,
    connectDisabledReason = null,
    addDisabledReason = null,
    showOfflineHint = false,
    pingEnabled = false,
    pingStates = {},
    onConnect,
    onSave,
    onSaveAndConnect,
    onDelete,
    onClose,
    onRefreshPings = undefined,
    onOpenPingSetting = undefined,
  }: {
    open: boolean;
    savedServers?: SavedServer[];
    savedServersLoading?: boolean;
    /** Set when the saved-server read FAILED. Distinct from an empty list: the
     *  dialog must not invite the user to re-add servers on top of a file it
     *  could not read. */
    savedServersError?: string | null;
    busy?: boolean;
    connectDisabledReason?: string | null;
    addDisabledReason?: string | null;
    showOfflineHint?: boolean;
    /** True when the user granted the server-status permission in Settings. */
    pingEnabled?: boolean;
    /** Per-address status, keyed by the same address string as the row. */
    pingStates?: Record<string, PingState>;
    onConnect: (address: string) => void;
    // Resolve `true` on a successful save so the dialog can clear + collapse.
    onSave: (name: string, address: string) => Promise<boolean>;
    onSaveAndConnect: (name: string, address: string) => Promise<boolean>;
    onDelete: (index: number, address: string) => void;
    onClose: () => void;
    onRefreshPings?: () => void;
    onOpenPingSetting?: () => void;
  } = $props();

  // A sweep is in flight while any row is still pending — used to keep repeated
  // Refresh clicks from stacking more dials onto the backend queue.
  const pingSweepRunning = $derived(Object.values(pingStates).some((s) => s === 'pending'));

  let name = $state('');
  let address = $state('');
  let touched = $state(false);
  let addOpen = $state(false);
  // Address awaiting delete confirmation, or null. Keyed by address (not list
  // index): after a delete the list shifts, so an index-keyed confirm would land
  // on whichever server slid into the freed slot and render *it* in confirm mode.
  let confirmingAddress = $state<string | null>(null);
  // Address just copied (drives the transient ✓ feedback), or null.
  let copiedAddress = $state<string | null>(null);

  const COPIED_FEEDBACK_MS = 1200;
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  function copyAddress(address: string) {
    void navigator.clipboard.writeText(address);
    copiedAddress = address;
    clearTimeout(copyTimer);
    copyTimer = setTimeout(() => {
      copiedAddress = null;
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
      confirmingAddress = null;
      copiedAddress = null;
      // Auto-expand Add only when the list is genuinely empty. After a failed
      // read `savedServers` is also [] — expanding then would present "you have
      // none, add one" as the answer to "we could not tell".
      addOpen = untrack(() => savedServers.length === 0 && savedServersError === null);
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

    {#if savedServersLoading}
      <LoadingPanel label={$t('quickJoin.loading')} />
    {:else if savedServersError}
      <p class="text-xs text-danger mb-3" role="alert" data-testid="quick-join-load-error">
        {$t('quickJoin.loadError', { error: savedServersError })}
      </p>
    {:else if savedServers.length > 0}
      <div class="flex items-center justify-between gap-2 mb-2">
        <p class="text-xs text-secondary">{$t('quickJoin.savedHeading')}</p>
        {#if pingEnabled}
          <button
            type="button"
            class="btn-ghost btn-sm"
            disabled={busy || pingSweepRunning}
            onclick={() => onRefreshPings?.()}
          >
            {$t('quickJoin.ping.refresh')}
          </button>
        {/if}
      </div>
      <!-- Permission off: say it plainly instead of leaving blank rows the user
           has to interpret, and offer the one click that changes it. -->
      {#if !pingEnabled}
        <p
          class="text-xs text-muted mb-2 flex flex-wrap items-center gap-1"
          data-testid="ping-disabled-notice"
        >
          <span>{$t('quickJoin.ping.disabledNotice')}</span>
          <button type="button" class="btn-ghost btn-sm" onclick={() => onOpenPingSetting?.()}>
            {$t('quickJoin.ping.enable')}
          </button>
        </p>
      {/if}
      <!-- Cap height + scroll so a long list stays inside the modal instead of
           overflowing the viewport. -->
      <ul class="flex flex-col gap-2 mb-3 max-h-72 overflow-y-auto pr-1">
        {#each savedServers as server, i (server.address)}
          <li
            class="flex items-center gap-2 bg-subtle border border-border-subtle rounded px-3 py-2"
          >
            {#if confirmingAddress === server.address}
              <span class="flex-1 text-xs text-danger min-w-0">
                {$t('quickJoin.deleteConfirm', { name: server.name })}
              </span>
              <button
                type="button"
                class="btn-secondary btn-sm"
                disabled={busy}
                onclick={() => (confirmingAddress = null)}
              >
                {$t('quickJoin.deleteCancel')}
              </button>
              <button
                type="button"
                class="btn-danger btn-sm"
                disabled={busy}
                onclick={() => {
                  // Reset the confirm flag before the list mutates, so the freed
                  // slot's new occupant never inherits the confirming state.
                  confirmingAddress = null;
                  onDelete(i, server.address);
                }}
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
                use:tooltip={$t(
                  copiedAddress === server.address ? 'quickJoin.copied' : 'quickJoin.copyAddress',
                )}
                onclick={() => copyAddress(server.address)}
              >
                <p class="text-sm font-medium text-primary truncate">{server.name}</p>
                <p class="text-xs text-secondary truncate flex items-center gap-1">
                  <span class="truncate">{server.address}</span>
                  {#if copiedAddress === server.address}<Icon name="success" size={12} />{/if}
                </p>
                <!-- Status line. Only rendered with the permission on, so a row
                     never shows a status-shaped blank the user must decode. -->
                {#if pingEnabled}
                  {@const state = pingStates[server.address]}
                  <span class="text-xs flex items-center gap-1 truncate" data-testid="ping-chip">
                    {#if state === 'pending'}
                      <span class="text-muted">{$t('quickJoin.ping.checking')}</span>
                    {:else if state?.kind === 'online'}
                      <span
                        class="text-success-text truncate"
                        use:tooltip={state.motd ?? undefined}
                      >
                        {formatPingChip(state)}
                      </span>
                    {:else if state?.kind === 'no_answer'}
                      <span class="text-muted">{$t('quickJoin.ping.noAnswer')}</span>
                    {/if}
                  </span>
                {/if}
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
                class="btn-icon btn-icon-sm btn-icon-danger"
                aria-label={$t('quickJoin.delete')}
                use:tooltip={$t('quickJoin.delete')}
                disabled={busy}
                onclick={() => (confirmingAddress = server.address)}
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
      <!--
        §5/§7: a native <details> reveals with the shared `.disclosure-caret`
        rotation, not an icon swap. `bind:open` stays — `addOpen` is written
        programmatically (the section auto-expands when there are no saved
        servers and collapses after a successful save) — it just no longer
        picks the glyph. Same shape as ImportPickerDialog and
        ServerPropertiesEditor.
      -->
      <summary class="flex items-center gap-2 cursor-pointer text-sm font-medium text-primary">
        <span class="disclosure-caret"><Icon name="caret" size={16} /></span>
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
              class="btn-success btn-sm w-full sm:w-auto"
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

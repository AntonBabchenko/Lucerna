<script lang="ts">
  // The Shared data section of the Settings tab (spec 2026-10-15): a one-time
  // copy of mod data from another instance (§A1), and the game settings this
  // instance shares with the others (§B9).
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import TransferDataDialog from '$lib/instances/transfer/TransferDataDialog.svelte';
  import SettingsSyncRows from './SettingsSyncRows.svelte';
  import { Icon } from '$lib/ui/icons';

  let {
    instance,
    instances,
    running,
    busy,
  }: {
    instance: InstanceWithStatus;
    instances: InstanceWithStatus[];
    /** THIS instance's game is up: nothing may be copied into it. */
    running: boolean;
    /** A task holds this instance. */
    busy: boolean;
  } = $props();

  let open = $state(false);
  const locked = $derived(running || busy);
</script>

<section class="mb-4 flex flex-col" data-testid="settings-shared-data">
  <h3 class="mb-2 text-sm font-medium text-primary">{$t('instance.transfer.sectionTitle')}</h3>
  <p class="mb-2 text-xs text-secondary">{$t('instance.transfer.sectionHint')}</p>
  <div>
    <button
      type="button"
      class="btn-secondary btn-sm inline-flex items-center gap-1.5"
      disabled={locked}
      aria-describedby={locked ? 'shared-data-locked' : undefined}
      onclick={() => (open = true)}
      data-testid="shared-data-copy"
    >
      <Icon name="copy" size={14} />
      {$t('instance.transfer.openBtn')}
    </button>
  </div>
  {#if locked}
    <p id="shared-data-locked" class="mt-1 text-xs text-muted" data-testid="shared-data-locked">
      {running ? $t('instance.transfer.lockedRunning') : $t('instance.transfer.lockedBusy')}
    </p>
  {/if}
  <SettingsSyncRows instanceId={instance.id} {running} {busy} />
</section>

{#if open}
  <TransferDataDialog target={instance} {instances} onClose={() => (open = false)} />
{/if}

<script lang="ts">
  // Server core switch (vanilla -> paper/purpur, or paper <-> purpur).
  // Single-phase: the backend (server_switch_core) owns the ENTIRE switch —
  // it takes its own mandatory backup, downloads the new core jar, and swaps
  // loader/loader_version. The UI must NOT call serverBackupCreate itself;
  // doing so would burn an extra KEEP_BACKUPS slot on a redundant snapshot.
  import { get } from 'svelte/store';
  import Modal from '$lib/ui/Modal.svelte';
  import Select, { type SelectOption } from '$lib/ui/Select.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { t } from '$lib/i18n';
  import { commands, type ServerCore } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { displayCore, pluginCapable, switchTargets } from '$lib/servers/core-display';

  let {
    serverId,
    currentCore,
    onClose,
  }: { serverId: string; currentCore: ServerCore; onClose: () => void } = $props();

  const targets = $derived(switchTargets(currentCore));
  const targetOptions = $derived<SelectOption[]>(
    targets.map((c) => ({ value: c, label: displayCore(c) })),
  );

  let target = $state<ServerCore | null>(null);
  $effect(() => {
    if (target === null && targets.length > 0) target = targets[0];
  });

  let switching = $state(false);
  let error = $state<string | null>(null);

  async function confirm() {
    if (target === null) return;
    error = null;
    switching = true;
    try {
      const res = await commands.serverSwitchCore(serverId, target);
      if (res.status === 'ok') {
        pushSuccess(get(t)('servers.core.switched', { target: displayCore(target) }));
        await serverState.refresh();
        onClose();
      } else {
        error = formatError(res.error);
      }
    } finally {
      switching = false;
    }
  }
</script>

<Modal
  ariaLabelledby="switch-core-title"
  {onClose}
  panelClass="w-[460px] p-5 flex flex-col gap-3"
  closeOnBackdrop={!switching}
  closeOnEscape={!switching}
>
  <h3 id="switch-core-title" class="text-base font-semibold text-primary">
    {$t('servers.core.modalTitle')}
  </h3>

  <label class="block">
    <span class="text-xs text-muted">{$t('servers.core.targetLabel')}</span>
    <Select
      class="mt-1 w-full"
      value={target}
      options={targetOptions}
      onChange={(v) => (target = v as ServerCore)}
      ariaLabel={$t('servers.core.targetLabel')}
      disabled={switching}
      dataTestid="switch-core-target"
    />
  </label>

  {#if currentCore === 'vanilla'}
    <p class="text-sm text-secondary">
      {$t('servers.core.warnConvert', { target: displayCore(target ?? currentCore) })}
    </p>
  {/if}

  {#if pluginCapable(currentCore)}
    <p class="text-sm text-secondary">{$t('servers.core.warnPaperFamily')}</p>
  {/if}

  <p class="rounded bg-subtle px-3 py-2 text-xs text-muted">
    {$t('servers.core.backupNote')}
  </p>

  {#if error}
    <p class="text-sm text-danger" data-testid="switch-core-error">{error}</p>
  {/if}

  <div class="mt-2 flex justify-end gap-2">
    <button type="button" class="btn-secondary btn-sm" onclick={onClose} disabled={switching}>
      {$t('common.cancel')}
    </button>
    <BusyButton
      class="btn-primary btn-sm"
      busy={switching}
      disabled={target === null}
      onclick={() => void confirm()}
    >
      {switching
        ? $t('servers.core.switching', { target: displayCore(target ?? currentCore) })
        : $t('servers.core.confirm')}
    </BusyButton>
  </div>
</Modal>

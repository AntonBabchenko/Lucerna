<script lang="ts">
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import MemorySlider from '$lib/instances/MemorySlider.svelte';
  import { formatHeapLabel } from '$lib/instances/heap';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { serversUi } from '$lib/servers/servers-ui.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import DeleteServerDialog from './DeleteServerDialog.svelte';

  let { serverId }: { serverId: string } = $props();

  const running = $derived(serverState.running(serverId));

  let name = $state('');
  let memoryMb = $state(4096);
  let jvmArgs = $state('');
  let initialized = false;

  $effect(() => {
    const s = serverState.list.find((x) => x.id === serverId);
    if (s && !initialized) {
      initialized = true;
      name = s.name;
      memoryMb = s.max_heap_mb;
      jvmArgs = s.extra_jvm_args;
    }
  });

  let busy = $state(false);
  let saved = $state(false);
  let error = $state<string | null>(null);

  const canSave = $derived(name.trim().length > 0);

  // Dirty guard (#34): the "Saved" confirmation must not linger once the user
  // starts editing again. We snapshot the fields on a successful save and clear
  // `saved` as soon as the live values diverge from that snapshot.
  const formSig = $derived(JSON.stringify({ name, memoryMb, jvmArgs }));
  let savedSnapshot = $state<string | null>(null);
  $effect(() => {
    if (saved && formSig !== savedSnapshot) saved = false;
  });

  async function save() {
    if (!canSave || busy) return;
    busy = true;
    error = null;
    saved = false;
    try {
      const r1 = await serverState.rename(serverId, name.trim());
      if (!r1.ok) {
        error = formatError(r1.error as Parameters<typeof formatError>[0]);
        return;
      }
      const r2 = await serverState.updateRuntimeConfig(serverId, memoryMb, jvmArgs);
      if (!r2.ok) {
        error = formatError(r2.error as Parameters<typeof formatError>[0]);
        return;
      }
      savedSnapshot = formSig;
      saved = true;
    } finally {
      busy = false;
    }
  }

  let confirmingDelete = $state(false);
  let deleteError = $state<string | null>(null);

  async function confirmDelete() {
    confirmingDelete = false;
    deleteError = null;
    const r = await serverState.remove(serverId);
    if (!r.ok) {
      deleteError = formatError(r.error as Parameters<typeof formatError>[0]);
      return;
    }
    // Fall back to the first remaining server (or the empty state).
    serversUi.selectServer(serverState.list[0]?.id ?? null);
  }
</script>

<div class="flex flex-col gap-4">
  <div class="flex flex-col gap-1">
    <label for="sg-name" class="text-sm font-medium">{$t('servers.general.name')}</label>
    <input
      id="sg-name"
      type="text"
      maxlength="32"
      class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
      bind:value={name}
    />
  </div>

  <div class="flex flex-col gap-1">
    <!-- svelte-ignore a11y_label_has_associated_control -->
    <label class="text-sm font-medium"
      >{$t('servers.general.memory')} · {formatHeapLabel(memoryMb)}</label
    >
    <MemorySlider valueMb={memoryMb} onInput={(mb) => (memoryMb = mb)} />
  </div>

  <div class="flex flex-col gap-1">
    <label for="sg-jvm" class="text-sm font-medium">{$t('servers.general.jvmArgs')}</label>
    <input
      id="sg-jvm"
      type="text"
      class="h-8 rounded border border-border-emphasis bg-surface px-3 font-mono text-xs text-primary"
      bind:value={jvmArgs}
    />
    <p class="text-xs text-muted">{$t('servers.general.jvmArgsHint')}</p>
  </div>

  {#if running}
    <p class="text-xs text-warning-text">{$t('servers.general.restartToApply')}</p>
  {/if}

  <div class="flex items-center gap-3">
    <BusyButton class="btn-primary btn-sm" {busy} disabled={!canSave} onclick={() => void save()}>
      {$t('servers.general.save')}
    </BusyButton>
    {#if saved}
      <span class="text-xs text-success">{$t('servers.general.saved')}</span>
    {/if}
    {#if error}
      <span class="text-xs text-danger">{error}</span>
    {/if}
  </div>

  <div class="border-t border-border-subtle pt-4 flex flex-col gap-2">
    <span class="text-sm font-medium text-danger">{$t('servers.general.dangerTitle')}</span>
    <span
      class="inline-flex self-start"
      use:tooltip={{
        text: running ? $t('servers.delete.runningBlock') : $t('servers.delete.trigger'),
        describe: false,
      }}
    >
      <button
        type="button"
        class="btn-danger btn-sm flex items-center gap-1.5"
        disabled={running}
        data-testid="server-delete-trigger"
        onclick={() => (confirmingDelete = true)}
      >
        <Icon name="trash" size={14} />
        {$t('servers.delete.trigger')}
      </button>
    </span>
    {#if deleteError}
      <span class="text-xs text-danger" role="alert">{deleteError}</span>
    {/if}
  </div>
</div>

{#if confirmingDelete}
  {@const s = serverState.list.find((x) => x.id === serverId)}
  <DeleteServerDialog
    serverName={s?.name ?? serverId}
    onCancel={() => (confirmingDelete = false)}
    onConfirm={() => void confirmDelete()}
  />
{/if}

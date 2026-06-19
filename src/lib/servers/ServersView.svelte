<script lang="ts">
  import { onMount } from 'svelte';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { Icon } from '$lib/ui/icons';
  import { displayLoader } from '$lib/instances/loader-display';
  import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/ipc/format-error';
  import ServerCreateWizard from './ServerCreateWizard.svelte';
  import ServerManageView from './ServerManageView.svelte';
  import DeleteServerDialog from './DeleteServerDialog.svelte';

  let { instances, versions }: { instances: InstanceWithStatus[]; versions: VersionEntry[] } =
    $props();
  let selected = $state<string | null>(null);
  let creating = $state(false);
  let pendingDelete = $state<{ id: string; name: string } | null>(null);
  let deleteError = $state<string | null>(null);

  onMount(() => {
    serverState.init();
    void serverState.refresh();
  });

  async function confirmDelete() {
    deleteError = null;
    const target = pendingDelete;
    if (!target) return;
    pendingDelete = null;
    const r = await serverState.remove(target.id);
    if (!r.ok) deleteError = formatError(r.error as Parameters<typeof formatError>[0]);
  }
</script>

{#if creating}
  <ServerCreateWizard
    {instances}
    {versions}
    onDone={() => {
      creating = false;
      void serverState.refresh();
    }}
    onCancel={() => (creating = false)}
  />
{:else if selected}
  {#key selected}
    <ServerManageView serverId={selected} onBack={() => (selected = null)} />
  {/key}
{:else}
  <div class="flex flex-col gap-3 p-4 overflow-y-auto">
    <div class="flex items-center justify-between">
      <span class="text-lg font-semibold">{$t('servers.title')}</span>
      <button
        type="button"
        class="btn-primary btn-sm flex items-center gap-1.5"
        onclick={() => (creating = true)}
      >
        <Icon name="plus" size={16} />
        {$t('servers.create')}
      </button>
    </div>
    {#if serverState.list.length === 0}
      <p class="text-muted text-sm">{$t('servers.empty')}</p>
    {:else}
      {#each serverState.list as s (s.id)}
        <div
          class="flex items-stretch gap-1 rounded-lg border border-border-subtle hover:border-accent"
        >
          <button
            type="button"
            class="flex flex-1 items-center gap-3 p-3 text-left"
            onclick={() => (selected = s.id)}
          >
            <Icon name="server" size={20} />
            <span class="flex-1">
              <span class="block font-medium">{s.name}</span>
              <span class="block text-xs text-muted"
                >{s.mc_version} · {displayLoader(s.loader)}{s.created_from_instance
                  ? ' · ' + s.created_from_instance
                  : ''}</span
              >
            </span>
            <span class="text-xs {s.running ? 'text-success' : 'text-muted'}">
              {s.running ? $t('servers.status.running') : $t('servers.status.stopped')}{s.port
                ? ' · ' + s.port
                : ''}
            </span>
            <Icon name="arrowRight" size={16} />
          </button>
          <button
            type="button"
            class="btn-icon-sm text-muted hover:text-danger disabled:opacity-40 disabled:cursor-not-allowed"
            aria-label={$t('servers.delete.trigger')}
            title={s.running ? $t('servers.delete.runningBlock') : $t('servers.delete.trigger')}
            disabled={s.running}
            onclick={() => (pendingDelete = { id: s.id, name: s.name })}
          >
            <Icon name="trash" size={16} />
          </button>
        </div>
      {/each}
    {/if}
    <p class="text-xs text-muted border border-dashed border-border-subtle rounded-lg p-3">
      {$t('servers.lanHint')}
    </p>
    {#if deleteError}
      <p class="text-sm text-danger">{deleteError}</p>
    {/if}
  </div>
  {#if pendingDelete}
    <DeleteServerDialog
      serverName={pendingDelete.name}
      onCancel={() => (pendingDelete = null)}
      onConfirm={() => void confirmDelete()}
    />
  {/if}
{/if}

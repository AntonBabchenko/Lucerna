<script lang="ts">
  import { onMount } from 'svelte';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { Icon } from '$lib/ui/icons';
  import { displayLoader } from '$lib/instances/loader-display';
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';

  let { instances }: { instances: InstanceWithStatus[] } = $props();
  let selected = $state<string | null>(null);
  let creating = $state(false);

  onMount(() => {
    serverState.init();
    void serverState.refresh();
  });
</script>

{#if creating}
  <!-- TODO(Task 6): <ServerCreateWizard {instances} onDone={() => { creating = false; void serverState.refresh(); }} onCancel={() => (creating = false)} /> -->
  <div class="p-4 text-muted">{$t('servers.create')}…</div>
{:else if selected}
  <!-- TODO(Task 7): <ServerManageView serverId={selected} onBack={() => (selected = null)} /> -->
  <div class="p-4">
    <button class="btn-tertiary btn-sm" onclick={() => (selected = null)}
      >&lt; {$t('servers.title')}</button
    >
    <p class="text-muted mt-2">{selected}</p>
  </div>
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
        <button
          type="button"
          class="flex items-center gap-3 rounded-lg border border-border-subtle p-3 text-left hover:border-accent"
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
      {/each}
    {/if}
    <p class="text-xs text-muted border border-dashed border-border-subtle rounded-lg p-3">
      {$t('servers.lanHint')}
    </p>
  </div>
{/if}

<script lang="ts">
  import { t } from '$lib/i18n';
  import { formatHeapLabel } from '$lib/instances/heap';
  import { displayCore } from '$lib/servers/core-display';
  import { serverState } from '$lib/servers/server-state.svelte';
  import ServerConsole from '$lib/servers/ServerConsole.svelte';
  import ServerConnectCard from './ServerConnectCard.svelte';

  // The servers-mode Overview: a card grid mirroring the client OverviewTab
  // (variant B of the consolidation spec). Server card = zero-IPC facts from
  // the already-loaded list row; Connection card = the former Connect tab;
  // Console card = ServerConsole embedded unchanged, full width.
  let { serverId }: { serverId: string } = $props();

  const server = $derived(serverState.list.find((s) => s.id === serverId) ?? null);
</script>

<div class="grid gap-4" style="grid-template-columns: repeat(2, minmax(0, 1fr));">
  {#if server}
    <div
      class="rounded-xl border border-border-subtle bg-surface p-3.5 flex flex-col gap-2"
      data-testid="server-overview-server-card"
    >
      <h3 class="text-[10px] uppercase tracking-wider text-muted">
        {$t('servers.overview.serverCard')}
      </h3>
      <div class="flex items-center justify-between text-sm">
        <span class="text-secondary">{$t('servers.overview.version')}</span>
        <span class="font-mono">{server.mc_version}</span>
      </div>
      <div class="flex items-center justify-between text-sm">
        <span class="text-secondary">{$t('servers.overview.core')}</span>
        <span class="font-mono">
          {displayCore(server.loader)}{server.loader_version ? ` (${server.loader_version})` : ''}
        </span>
      </div>
      <div class="flex items-center justify-between text-sm">
        <span class="text-secondary">{$t('servers.overview.memory')}</span>
        <span class="font-mono">{formatHeapLabel($t, server.max_heap_mb)}</span>
      </div>
    </div>

    <ServerConnectCard {serverId} />

    <div
      class="rounded-xl border border-border-subtle bg-surface p-3.5 flex flex-col gap-2"
      style="grid-column: 1 / -1;"
      data-testid="server-overview-console-card"
    >
      <h3 class="text-[10px] uppercase tracking-wider text-muted">
        {$t('servers.overview.consoleCard')}
      </h3>
      <ServerConsole {serverId} />
    </div>
  {/if}
</div>

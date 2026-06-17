<script lang="ts">
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { Icon } from '$lib/ui/icons';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import ServerConsole from './ServerConsole.svelte';
  import ServerSettings from './ServerSettings.svelte';
  import ServerMods from './ServerMods.svelte';

  let { serverId, onBack }: { serverId: string; onBack: () => void } = $props();

  type ServerTab = 'console' | 'settings' | 'mods';

  const server = $derived(serverState.list.find((s) => s.id === serverId));
  const running = $derived(serverState.running(serverId));

  let tab = $state<ServerTab>('console');
  let busyStart = $state(false);
  let busyStop = $state(false);
  let busyRestart = $state(false);
  let actionError = $state<string | null>(null);

  async function start() {
    busyStart = true;
    actionError = null;
    try {
      const res = await commands.serverStart(serverId);
      if (res.status !== 'ok') actionError = formatError(res.error);
      else await serverState.refresh();
    } finally {
      busyStart = false;
    }
  }

  async function stop() {
    busyStop = true;
    actionError = null;
    try {
      const res = await commands.serverStop(serverId);
      if (res.status !== 'ok') actionError = formatError(res.error);
      else await serverState.refresh();
    } finally {
      busyStop = false;
    }
  }

  async function restart() {
    busyRestart = true;
    actionError = null;
    try {
      const res = await commands.serverRestart(serverId);
      if (res.status !== 'ok') actionError = formatError(res.error);
      else await serverState.refresh();
    } finally {
      busyRestart = false;
    }
  }
</script>

<div class="flex flex-col overflow-hidden h-full">
  <!-- Header -->
  <div class="flex items-center gap-3 border-b border-border-subtle px-4 py-2">
    <button type="button" class="btn-ghost btn-sm flex items-center gap-1" onclick={onBack}>
      <Icon name="arrowLeft" size={14} />
      {$t('servers.manage.back')}
    </button>

    <span class="flex-1 font-semibold truncate">{server?.name ?? serverId}</span>

    <!-- Status pill -->
    <span
      class="rounded-full px-2 py-0.5 text-xs font-medium {running
        ? 'bg-success/15 text-success'
        : 'bg-muted/15 text-muted'}"
    >
      {running ? $t('servers.status.running') : $t('servers.status.stopped')}{server?.port
        ? ' · ' + server.port
        : ''}
    </span>

    <!-- Actions -->
    <div class="flex items-center gap-1.5">
      {#if !running}
        <BusyButton class="btn-primary btn-sm" busy={busyStart} onclick={() => void start()}>
          {$t('servers.action.start')}
        </BusyButton>
      {:else}
        <BusyButton class="btn-ghost btn-sm" busy={busyRestart} onclick={() => void restart()}>
          {$t('servers.action.restart')}
        </BusyButton>
        <BusyButton class="btn-ghost btn-sm" busy={busyStop} onclick={() => void stop()}>
          {$t('servers.action.stop')}
        </BusyButton>
      {/if}
    </div>
  </div>

  {#if actionError}
    <p class="px-4 pt-2 text-sm text-danger">{actionError}</p>
  {/if}

  <!-- Sub-tabs -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div role="tablist" class="flex gap-1 border-b border-border-subtle px-4 bg-surface">
    {#each [['console', $t('servers.tab.console')], ['settings', $t('servers.tab.settings')], ['mods', $t('servers.tab.mods')]] as const as [id, label] (id)}
      <button
        type="button"
        role="tab"
        aria-selected={tab === id}
        tabindex={tab === id ? 0 : -1}
        class="px-3 py-2 text-sm border-b-2 -mb-px transition-colors"
        class:border-accent={tab === id}
        class:text-primary={tab === id}
        class:font-semibold={tab === id}
        class:border-transparent={tab !== id}
        class:text-muted={tab !== id}
        onclick={() => (tab = id)}
      >
        {label}
      </button>
    {/each}
  </div>

  <!-- Tab body -->
  <div class="flex-1 overflow-y-auto p-4">
    {#if tab === 'console'}
      <ServerConsole {serverId} />
    {:else if tab === 'settings'}
      <ServerSettings {serverId} />
    {:else}
      <ServerMods {serverId} />
    {/if}
  </div>
</div>

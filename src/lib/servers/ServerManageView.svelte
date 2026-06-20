<script lang="ts">
  import { onMount } from 'svelte';
  import { commands, type ServerWithStatus_Serialize } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { Icon } from '$lib/ui/icons';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import ServerConsole from './ServerConsole.svelte';
  import ServerGeneralSettings from './ServerGeneralSettings.svelte';
  import ServerSettings from './ServerSettings.svelte';
  import ServerMods from './ServerMods.svelte';
  import ServerDiagnosisBanner from './ServerDiagnosisBanner.svelte';
  import ServerHostingTab from './ServerHostingTab.svelte';
  import ServerToInstanceDialog from './ServerToInstanceDialog.svelte';

  let {
    serverId,
    onBack,
    onInstanceCreated,
  }: {
    serverId: string;
    onBack: () => void;
    onInstanceCreated: (instanceId: string) => void;
  } = $props();

  type ServerTab = 'console' | 'general' | 'settings' | 'mods' | 'hosting';

  // serverList() always returns ServerWithStatus_Serialize[]; the store type
  // is the union for legacy reasons. Cast here so the dialog prop is satisfied.
  const server = $derived(
    serverState.list.find((s) => s.id === serverId) as ServerWithStatus_Serialize | undefined,
  );
  const running = $derived(serverState.running(serverId));

  let tab = $state<ServerTab>('console');
  let showToInstance = $state(false);
  let busyStart = $state(false);
  let busyStop = $state(false);
  let busyRestart = $state(false);
  let actionError = $state<string | null>(null);

  // Diagnose on mount and whenever the server transitions from running → stopped.
  onMount(() => {
    void serverState.diagnose(serverId);
  });

  // Re-diagnose when the server stops (running → false transition).
  // We track the previous value in a $state variable so the $effect
  // can read both the old and new value reactively.
  // svelte-ignore state_referenced_locally
  let _prevRunning = $state(serverState.running(serverId));
  $effect(() => {
    const isRunning = serverState.running(serverId);
    if (_prevRunning && !isRunning) {
      void serverState.diagnose(serverId);
    }
    _prevRunning = isRunning;
  });

  async function start() {
    busyStart = true;
    actionError = null;
    try {
      const res = await commands.serverStart(serverId);
      if (res.status !== 'ok') {
        actionError = formatError(res.error);
        // Surface the rich fixable banner (orphan / port / EULA) for this failure.
        void serverState.diagnose(serverId);
      } else await serverState.refresh();
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
      <button
        type="button"
        class="btn-ghost btn-sm flex items-center gap-1"
        onclick={() => (showToInstance = true)}
        data-testid="create-client-instance-btn"
      >
        <Icon name="download" size={14} />
        {$t('servers.toInstance.button')}
      </button>
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

  <!-- Inline action error is the UNCLASSIFIED fallback only: when start()'s
       diagnose() produced a rich banner for this server, the banner owns the
       message and we suppress this duplicate. -->
  {#if actionError && !serverState.diagnosisFor(serverId)}
    <p class="px-4 pt-2 text-sm text-danger" role="alert" data-testid="server-action-error">
      {actionError}
    </p>
  {/if}

  <!-- Diagnosis banner (shown when the server crash-diagnosed after stop) -->
  <div class="px-4 pt-2">
    <ServerDiagnosisBanner {serverId} />
  </div>

  <!-- Sub-tabs -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div role="tablist" class="flex gap-1 border-b border-border-subtle px-4 bg-surface">
    {#each [['console', $t('servers.tab.console')], ['general', $t('servers.tab.general')], ['settings', $t('servers.tab.settings')], ['mods', $t('servers.tab.mods')], ['hosting', $t('servers.hosting.tab')]] as const as [id, label] (id)}
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
    {:else if tab === 'general'}
      <ServerGeneralSettings {serverId} />
    {:else if tab === 'settings'}
      <ServerSettings {serverId} />
    {:else if tab === 'mods'}
      <ServerMods {serverId} />
    {:else}
      <ServerHostingTab {serverId} />
    {/if}
  </div>
</div>

{#if showToInstance && server}
  <ServerToInstanceDialog
    {server}
    onCancel={() => (showToInstance = false)}
    onCreated={(id) => {
      showToInstance = false;
      onInstanceCreated(id);
    }}
  />
{/if}

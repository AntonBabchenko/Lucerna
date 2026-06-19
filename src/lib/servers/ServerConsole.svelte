<script lang="ts">
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import type { ServerLogInfo } from '$lib/ipc/bindings';
  import { serverState } from '$lib/servers/server-state.svelte';
  import Select from '$lib/ui/Select.svelte';
  import type { SelectOption } from '$lib/ui/Select.svelte';

  let { serverId }: { serverId: string } = $props();

  // ---------------------------------------------------------------------------
  // Live-console state
  // ---------------------------------------------------------------------------
  const lines = $derived(serverState.lines(serverId));
  const running = $derived(serverState.running(serverId));

  let draft = $state('');
  let busy = $state(false);
  let sendError = $state<string | null>(null);
  let container = $state<HTMLDivElement | null>(null);

  $effect(() => {
    // Read lines.length to make this effect re-run whenever new lines arrive.
    void lines.length;
    if (container && archivedText === null) {
      container.scrollTop = container.scrollHeight;
    }
  });

  async function send() {
    const cmd = draft.trim();
    if (!cmd || busy || !running) return;
    busy = true;
    sendError = null;
    try {
      const res = await commands.serverSendCommand(serverId, cmd);
      if (res.status === 'ok') {
        draft = '';
      } else {
        sendError = formatError(res.error);
      }
    } finally {
      busy = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void send();
    }
  }

  // ---------------------------------------------------------------------------
  // Past-sessions log picker state
  // ---------------------------------------------------------------------------
  let logs = $state<ServerLogInfo[]>([]);
  let selectedFile = $state<string | null>(null);
  let archivedText = $state<string | null>(null);
  let loadingText = $state(false);
  let prevRunning = $state<boolean | null>(null);

  const LIVE_VALUE = '__live__';

  async function loadLogs(): Promise<void> {
    const r = await serverState.listLogs(serverId);
    if (r.ok && r.list) {
      logs = r.list;
    }
  }

  // Load on mount.
  $effect(() => {
    void loadLogs();
  });

  // Reload when the server transitions from running → stopped (a new archive was just created).
  $effect(() => {
    if (prevRunning === true && !running) {
      void loadLogs();
    }
    prevRunning = running;
  });

  // Build picker options: "Current session" + one entry per archive.
  const pickerOptions = $derived<SelectOption[]>([
    { value: LIVE_VALUE, label: $t('servers.logs.latest') },
    ...logs.filter((l) => !l.is_latest).map((l) => ({ value: l.file_name, label: l.file_name })),
  ]);

  async function onPickerChange(value: string | number): Promise<void> {
    const file = String(value);
    selectedFile = file === LIVE_VALUE ? null : file;

    if (selectedFile === null) {
      archivedText = null;
      return;
    }

    loadingText = true;
    try {
      const r = await serverState.readLog(serverId, selectedFile);
      archivedText = r.ok && r.text !== undefined ? r.text : null;
    } finally {
      loadingText = false;
    }
  }

  function backToLive(): void {
    selectedFile = null;
    archivedText = null;
  }

  const archives = $derived(logs.filter((l) => !l.is_latest));
</script>

<div class="flex flex-col gap-2">
  <!-- Log controls row: open-folder + past-sessions picker -->
  <div class="flex items-center gap-2">
    <button
      type="button"
      class="btn-ghost btn-sm shrink-0"
      onclick={() => void serverState.openLogsFolder(serverId)}
    >
      {$t('servers.logs.openFolder')}
    </button>

    {#if archives.length > 0}
      <div class="flex-1">
        <Select
          value={selectedFile ?? LIVE_VALUE}
          options={pickerOptions}
          onChange={onPickerChange}
          ariaLabel={$t('servers.logs.pastSessions')}
        />
      </div>
    {/if}
  </div>

  <!-- Console body: live stream OR read-only archive viewer -->
  {#if archivedText !== null}
    <!-- Read-only archive viewer -->
    <div
      class="h-80 overflow-y-auto rounded border border-border-subtle bg-base p-2 font-mono text-xs"
    >
      {#if loadingText}
        <span class="text-muted">{$t('common.loading')}</span>
      {:else if archivedText.length === 0}
        <span class="text-muted">{$t('servers.logs.noLogs')}</span>
      {:else}
        <div class="whitespace-pre-wrap break-all leading-5">{archivedText}</div>
      {/if}
    </div>

    <div class="flex items-center gap-2">
      <button type="button" class="btn-ghost btn-sm" onclick={backToLive}>
        {$t('servers.logs.backToLive')}
      </button>
    </div>
  {:else}
    <!-- Live console -->
    <div
      bind:this={container}
      class="h-80 overflow-y-auto rounded border border-border-subtle bg-base p-2 font-mono text-xs"
    >
      {#if lines.length === 0}
        <span class="text-muted">{$t('servers.console.empty')}</span>
      {:else}
        {#each lines as line, i (i)}
          <div class="whitespace-pre-wrap break-all leading-5">{line}</div>
        {/each}
      {/if}
    </div>

    {#if !running}
      <p class="text-xs text-muted">{$t('servers.console.notRunning')}</p>
    {/if}

    {#if sendError}
      <p class="text-xs text-danger">{sendError}</p>
    {/if}

    <div class="flex gap-2">
      <input
        type="text"
        class="h-8 flex-1 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary disabled:opacity-50"
        placeholder={$t('servers.console.inputPlaceholder')}
        bind:value={draft}
        disabled={!running || busy}
        onkeydown={onKeydown}
      />
      <button
        type="button"
        class="btn-primary btn-sm"
        disabled={!running || busy || draft.trim().length === 0}
        onclick={() => void send()}
      >
        {$t('servers.console.send')}
      </button>
    </div>
  {/if}
</div>

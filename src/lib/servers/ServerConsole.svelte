<script lang="ts">
  import { get } from 'svelte/store';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import type { ServerLogInfo } from '$lib/ipc/bindings';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Select from '$lib/ui/Select.svelte';
  import type { SelectOption } from '$lib/ui/Select.svelte';

  // The live `server-latest.log` filename (matches serverlog::LATEST on the backend).
  const LATEST_LOG = 'server-latest.log';

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
  // Tail of `server-latest.log`, shown in the live view when we have NO captured
  // output (panel reopened, or the launcher was restarted while the server kept
  // running so its stdout isn't being streamed). Live events take over once they
  // arrive.
  let backfillText = $state<string | null>(null);

  $effect(() => {
    // Read lines.length / backfillText to re-run whenever new output appears.
    void lines.length;
    void backfillText;
    if (container && archivedText === null) {
      container.scrollTop = container.scrollHeight;
    }
  });

  // Load the backfill once per server while the live buffer is empty + running.
  $effect(() => {
    const hasLive = serverState.lines(serverId).length > 0;
    const isRunning = serverState.running(serverId);
    if (isRunning && !hasLive && backfillText === null) {
      void commands.serverReadLog(serverId, LATEST_LOG).then((r) => {
        if (r.status === 'ok') backfillText = r.data;
      });
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
  let readError = $state<string | null>(null);
  let loadingText = $state(false);
  let prevRunning: boolean | null = null;

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
    const cur = serverState.running(serverId);
    if (prevRunning === true && !cur) void loadLogs();
    prevRunning = cur;
  });

  // Reset archive view when serverId changes so stale content can't linger.
  $effect(() => {
    void serverId;
    selectedFile = null;
    archivedText = null;
    readError = null;
    backfillText = null;
  });

  // Build picker options: "Current session" + one entry per archive.
  const pickerOptions = $derived<SelectOption[]>([
    { value: LIVE_VALUE, label: $t('servers.logs.latest') },
    ...logs.filter((l) => !l.is_latest).map((l) => ({ value: l.file_name, label: l.file_name })),
  ]);

  async function onPickerChange(value: string | number): Promise<void> {
    const file = String(value);
    readError = null;

    if (file === LIVE_VALUE) {
      selectedFile = null;
      archivedText = null;
      return;
    }

    selectedFile = file;
    archivedText = null;
    loadingText = true;
    try {
      const r = await commands.serverReadLog(serverId, selectedFile);
      if (r.status === 'ok') {
        archivedText = r.data;
      } else {
        readError = formatError(r.error);
      }
    } finally {
      loadingText = false;
    }
  }

  function backToLive(): void {
    selectedFile = null;
    archivedText = null;
    readError = null;
  }

  // ---------------------------------------------------------------------------
  // Share to mclo.gs (reuses the instance share command — content is anonymised
  // server-side before upload). Shares the archive being viewed, or the latest
  // session when on the live console.
  // ---------------------------------------------------------------------------
  let busyShare = $state(false);
  let shareUrl = $state<string | null>(null);
  let shareError = $state<string | null>(null);

  async function shareLog(): Promise<void> {
    busyShare = true;
    shareError = null;
    shareUrl = null;
    try {
      // Prefer the already-loaded archive text; otherwise read the latest log.
      let content = archivedText;
      if (content === null) {
        const r = await commands.serverReadLog(serverId, LATEST_LOG);
        if (r.status !== 'ok') {
          shareError = formatError(r.error);
          return;
        }
        content = r.data;
      }
      if (!content || content.length === 0) {
        shareError = get(t)('servers.logs.shareEmpty');
        return;
      }
      const res = await commands.shareLogToMclogs(content);
      if (res.status === 'ok') {
        shareUrl = res.data;
        // Best-effort copy; the URL is also shown for manual copy.
        try {
          await navigator.clipboard?.writeText(res.data);
        } catch {
          // clipboard unavailable — the URL is still displayed below
        }
        pushSuccess(get(t)('servers.logs.shareCopied'));
      } else {
        shareError = formatError(res.error);
      }
    } finally {
      busyShare = false;
    }
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

    <BusyButton
      class="btn-ghost btn-sm shrink-0"
      data-testid="server-log-share"
      busy={busyShare}
      onclick={() => void shareLog()}
    >
      {$t('servers.logs.share')}
    </BusyButton>

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

  {#if shareError}
    <p class="text-xs text-danger" role="alert" data-testid="server-log-share-error">
      {shareError}
    </p>
  {/if}
  {#if shareUrl}
    <p class="text-xs text-secondary" data-testid="server-log-share-url">
      {$t('servers.logs.sharedAt')}
      <span class="select-all break-all font-mono text-primary">{shareUrl}</span>
    </p>
  {/if}

  <!-- Console body: live stream OR read-only archive viewer -->
  {#if selectedFile !== null}
    <!-- Read-only archive viewer -->
    <p class="text-xs text-muted">{$t('servers.logs.viewing', { name: selectedFile })}</p>

    <div
      class="h-80 overflow-y-auto rounded border border-border-subtle bg-base p-2 font-mono text-xs"
    >
      {#if loadingText}
        <span class="text-muted">{$t('common.loading')}</span>
      {:else if readError}
        <span class="text-danger">{readError}</span>
      {:else if archivedText !== null && archivedText.length === 0}
        <span class="text-muted">{$t('servers.logs.noLogs')}</span>
      {:else if archivedText !== null}
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
      {#if lines.length > 0}
        {#each lines as line, i (i)}
          <div class="whitespace-pre-wrap break-all leading-5">{line}</div>
        {/each}
      {:else if backfillText && backfillText.length > 0}
        <!-- No live capture — show the saved log so a running server isn't blank. -->
        <div class="whitespace-pre-wrap break-all leading-5 text-secondary">{backfillText}</div>
      {:else}
        <span class="text-muted">{$t('servers.console.empty')}</span>
      {/if}
    </div>

    {#if !running}
      <p class="text-xs text-muted">{$t('servers.console.notRunning')}</p>
    {/if}

    <!-- Console-local: a failed chat/command send, not a launch failure the
         diagnosis banner can classify, so it stays inline here by design. -->
    {#if sendError}
      <p class="text-xs text-danger" role="alert" data-testid="server-send-error">{sendError}</p>
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

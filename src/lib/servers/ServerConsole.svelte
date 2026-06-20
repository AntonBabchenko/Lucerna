<script lang="ts">
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';

  let { serverId }: { serverId: string } = $props();

  const lines = $derived(serverState.lines(serverId));
  const running = $derived(serverState.running(serverId));

  let draft = $state('');
  let busy = $state(false);
  let sendError = $state<string | null>(null);
  let container = $state<HTMLDivElement | null>(null);

  $effect(() => {
    // Read lines.length to make this effect re-run whenever new lines arrive.
    void lines.length;
    if (container) {
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
</script>

<div class="flex flex-col gap-2">
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
</div>

<script lang="ts">
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import type { ServerConnectivity } from '$lib/ipc/bindings';

  let { serverId }: { serverId: string } = $props();

  let snapshot = $state<ServerConnectivity | null>(null);
  let copiedAddr = $state<string | null>(null);

  // Fetch connectivity whenever running state changes.
  // svelte-ignore state_referenced_locally
  let _prevRunning = $state(serverState.running(serverId));
  $effect(() => {
    const isRunning = serverState.running(serverId);
    if (isRunning !== _prevRunning || (isRunning && snapshot === null)) {
      _prevRunning = isRunning;
      void serverState.connectivity(serverId).then((r) => {
        snapshot = r;
      });
    }
    _prevRunning = isRunning;
  });

  function joinAddress(addr: string): string {
    const port = snapshot?.port ?? 25565;
    return `${addr}:${port}`;
  }

  async function copyInvite(addr: string): Promise<void> {
    const text = joinAddress(addr);
    void navigator.clipboard.writeText(text);
    copiedAddr = addr;
    setTimeout(() => {
      copiedAddr = null;
    }, 1500);
  }

  const running = $derived(serverState.running(serverId));
</script>

<div class="flex flex-col gap-4 text-sm">
  {#if !running}
    <p class="text-muted">{$t('servers.connect.stoppedHint')}</p>
  {:else}
    <div class="flex flex-col gap-3">
      <!-- LAN section -->
      <div>
        <h3 class="font-semibold mb-1">{$t('servers.connect.lanTitle')}</h3>
        <p class="text-muted mb-2">{$t('servers.connect.lanHint')}</p>

        {#if snapshot && snapshot.lan_addresses.length > 0}
          <div class="flex flex-col gap-2">
            {#each snapshot.lan_addresses as addr (addr)}
              <div class="flex items-center gap-2">
                <code class="rounded bg-subtle px-2 py-1 font-mono text-xs">{joinAddress(addr)}</code>
                <button
                  type="button"
                  class="btn-ghost btn-sm"
                  onclick={() => void copyInvite(addr)}
                >
                  {copiedAddr === addr ? $t('servers.connect.copied') : $t('servers.connect.copyInvite')}
                </button>
              </div>
            {/each}
          </div>
        {:else}
          <p class="text-muted">{$t('servers.connect.noLan')}</p>
        {/if}
      </div>

      <!-- Hints -->
      <p class="text-muted text-xs">{$t('servers.connect.localHint')}</p>
      <p class="text-muted text-xs">{$t('servers.connect.internetHint')}</p>

      <!-- Online-mode explainer -->
      {#if snapshot}
        <p class={snapshot.online_mode ? 'text-muted text-xs' : 'text-warning text-xs'}>
          {snapshot.online_mode ? $t('servers.connect.onlineModeOn') : $t('servers.connect.onlineModeOff')}
        </p>
      {/if}
    </div>
  {/if}
</div>

<script lang="ts">
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { commands } from '$lib/ipc/bindings';
  import type { FirewallState, ServerConnectivity } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import { setProperty } from './properties-edit';
  import BusyButton from '$lib/ui/BusyButton.svelte';

  let { serverId }: { serverId: string } = $props();

  let snapshot = $state<ServerConnectivity | null>(null);
  let copiedAddr = $state<string | null>(null);
  // The address whose copy attempt failed (clipboard blocked/unavailable). We
  // never claim "Copied!" then — the user is told to select + Ctrl+C instead.
  let copyFailedAddr = $state<string | null>(null);
  let firewall = $state<FirewallState | null>(null);
  let addingRule = $state(false);
  let addRuleOutcome = $state<string | null>(null);
  let switchingOffline = $state(false);
  let offlineError = $state<string | null>(null);

  // Turn off online-mode so offline accounts can join, then restart to apply
  // (server.properties is only read at startup). Mirrors ServerSettings' RMW.
  async function allowOffline(): Promise<void> {
    switchingOffline = true;
    offlineError = null;
    try {
      const r = await commands.serverReadProperties(serverId);
      if (r.status !== 'ok') {
        offlineError = formatError(r.error as Parameters<typeof formatError>[0]);
        return;
      }
      let next = setProperty(r.data, 'online-mode', 'false');
      // With online-mode off, secure-profile enforcement is meaningless and can
      // kick offline clients — turn it off too.
      next = setProperty(next, 'enforce-secure-profile', 'false');
      const w = await commands.serverWriteProperties(serverId, next);
      if (w.status !== 'ok') {
        offlineError = formatError(w.error as Parameters<typeof formatError>[0]);
        return;
      }
      if (serverState.running(serverId)) {
        const rs = await commands.serverRestart(serverId);
        if (rs.status !== 'ok') {
          offlineError = formatError(rs.error as Parameters<typeof formatError>[0]);
          return;
        }
      }
      snapshot = await serverState.connectivity(serverId);
      pushSuccess(get(t)('servers.connect.offlineEnabled'));
    } finally {
      switchingOffline = false;
    }
  }

  // Fetch connectivity and firewall status whenever running state changes.
  // svelte-ignore state_referenced_locally
  let _prevRunning = $state(serverState.running(serverId));
  $effect(() => {
    const isRunning = serverState.running(serverId);
    if (isRunning !== _prevRunning || (isRunning && snapshot === null)) {
      void serverState.connectivity(serverId).then((r) => {
        snapshot = r;
      });
      if (isRunning) {
        void serverState.firewallStatus(serverId).then((s) => {
          firewall = s;
        });
      } else {
        firewall = null;
        addRuleOutcome = null;
      }
    }
    _prevRunning = isRunning;
  });

  function joinAddress(addr: string): string {
    const port = snapshot?.port ?? 25565;
    return `${addr}:${port}`;
  }

  async function copyInvite(addr: string): Promise<void> {
    const text = joinAddress(addr);
    copyFailedAddr = null;
    try {
      await navigator.clipboard.writeText(text);
      copiedAddr = addr;
      setTimeout(() => {
        copiedAddr = null;
      }, 1500);
    } catch {
      // Clipboard write rejected (permissions / unavailable) — be honest rather
      // than flashing "Copied!"; point the user at the selectable address.
      copyFailedAddr = addr;
    }
  }

  async function handleAddRule(): Promise<void> {
    addingRule = true;
    addRuleOutcome = null;
    try {
      const result = await serverState.firewallAddRule(serverId);
      if (result.ok) {
        addRuleOutcome = $t('servers.connect.firewall.added');
        // Re-fetch firewall status after a short delay to pick up the new rule.
        setTimeout(() => {
          void serverState.firewallStatus(serverId).then((s) => {
            firewall = s;
          });
        }, 1200);
      } else {
        addRuleOutcome = formatError(result.error as Parameters<typeof formatError>[0]);
      }
    } finally {
      addingRule = false;
    }
  }

  const running = $derived(serverState.running(serverId));
  const port = $derived(snapshot?.port ?? 25565);
</script>

<div class="flex flex-col gap-4 text-sm">
  {#if !running}
    <p class="text-muted">{$t('servers.connect.stoppedHint')}</p>
  {:else}
    <div class="flex flex-col gap-3">
      <!-- Firewall banner — only on Windows when a rule is missing -->
      {#if firewall === 'needs_rule'}
        <div class="rounded border border-warning/40 bg-warning/10 p-3 flex flex-col gap-2">
          <p class="font-semibold text-warning">{$t('servers.connect.firewall.blockedTitle')}</p>
          <p class="text-muted text-xs">
            {$t('servers.connect.firewall.blockedHint', { port })}
          </p>

          <div class="flex items-center gap-2">
            <BusyButton
              busy={addingRule}
              class="btn-warning btn-sm"
              onclick={() => void handleAddRule()}
            >
              {$t('servers.connect.firewall.addRule')}
            </BusyButton>
            {#if addRuleOutcome}
              <span class="text-xs text-muted">{addRuleOutcome}</span>
            {/if}
          </div>

          <details class="text-xs">
            <summary class="cursor-pointer text-muted hover:text-foreground">
              {$t('servers.connect.firewall.manual')}
            </summary>
            <p class="mt-1 text-muted">{$t('servers.connect.firewall.manualSteps')}</p>
            <code class="mt-1 block rounded bg-subtle px-2 py-1 font-mono break-all">
              netsh advfirewall firewall add rule name="Lucerna Minecraft Server (TCP {port})"
              dir=in action=allow protocol=TCP localport={port}
            </code>
          </details>
        </div>
      {:else if firewall === 'allowed'}
        <p class="text-xs text-muted">{$t('servers.connect.firewall.allowed')}</p>
      {/if}

      <!-- LAN section -->
      <div>
        <h3 class="font-semibold mb-1">{$t('servers.connect.lanTitle')}</h3>
        <p class="text-muted mb-2">{$t('servers.connect.lanHint')}</p>

        {#if snapshot === null}
          <!-- Waiting for connectivity snapshot — show nothing to avoid a misleading flash -->
        {:else if snapshot.lan_addresses.length > 0}
          <div class="flex flex-col gap-2">
            {#each snapshot.lan_addresses as addr (addr)}
              <div class="flex flex-col gap-1">
                <div class="flex items-center gap-2">
                  <code class="select-all rounded bg-subtle px-2 py-1 font-mono text-xs"
                    >{joinAddress(addr)}</code
                  >
                  <button
                    type="button"
                    class="btn-ghost btn-sm"
                    onclick={() => void copyInvite(addr)}
                  >
                    {copiedAddr === addr
                      ? $t('servers.connect.copied')
                      : $t('servers.connect.copyInvite')}
                  </button>
                </div>
                {#if copyFailedAddr === addr}
                  <span
                    class="text-xs text-warning-text"
                    role="alert"
                    data-testid="copy-invite-failed"
                  >
                    {$t('servers.connect.copyFailed')}
                  </span>
                {/if}
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
          {snapshot.online_mode
            ? $t('servers.connect.onlineModeOn')
            : $t('servers.connect.onlineModeOff')}
        </p>
        {#if snapshot.online_mode}
          <!-- One-click fix for the most common "Invalid session" wall: let
               offline accounts join. Restarts the server to apply. -->
          <div class="flex items-center gap-2">
            <BusyButton
              class="btn-ghost btn-sm"
              data-testid="server-allow-offline"
              busy={switchingOffline}
              onclick={() => void allowOffline()}
            >
              {$t('servers.connect.allowOffline')}
            </BusyButton>
            {#if offlineError}
              <span class="text-xs text-danger" role="alert">{offlineError}</span>
            {/if}
          </div>
        {/if}
      {/if}
    </div>
  {/if}
</div>

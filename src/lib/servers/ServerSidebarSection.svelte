<!-- src/lib/servers/ServerSidebarSection.svelte -->
<script lang="ts">
  import type { Error as IpcError, ServerWithStatus } from '$lib/ipc/bindings';
  import { Icon } from '$lib/ui/icons';
  import Select, { type SelectOption } from '$lib/ui/Select.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import NavStatusIcon from '$lib/layout/NavStatusIcon.svelte';
  import { navVisual } from '$lib/layout/nav-status';
  import { serverState, serverNavStatus } from '$lib/servers/server-state.svelte';
  import { serversUi } from '$lib/servers/servers-ui.svelte';
  import { displayCore, modCapable, pluginCapable } from '$lib/servers/core-display';
  import { compactState, setCompact } from '$lib/layout/compact.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { t } from '$lib/i18n';
  import { get } from 'svelte/store';
  import { formatError } from '$lib/ipc/format-error';
  import { pushWarning } from '$lib/toasts/toasts.svelte';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import { dataRootCreateDisabledKey } from '$lib/settings/data-root-gating';

  // Servers-mode middle of the sidebar: the mirror of the instance section.
  // Selector (status icon per row) + create + the big Start/Stop. All server
  // data comes from the global serverState store; selection lives in serversUi,
  // so this component needs no props at all.
  const selected = $derived(
    serverState.list.find((s) => s.id === serversUi.selectedServerId) ?? null,
  );
  const serverOptions = $derived(
    serverState.list.map((s) => ({
      value: s.id,
      label: `${s.name} · ${s.mc_version} · ${displayCore(s.loader)}`,
    })),
  );
  const action = $derived(selected ? serverState.actionFor(selected.id) : null);
  const uploading = $derived(selected ? serverState.isUploading(selected.id) : false);
  const killing = $derived(selected ? serverState.isKilling(selected.id) : false);

  // The "Force stop now" escalation appears a few seconds into a graceful stop,
  // so a normal quick shutdown never flashes it — it only surfaces when the wait
  // actually drags (a still-loading or hung server that never processed `stop`).
  // Reset the instant the stop ends or the timer is superseded.
  const FORCE_STOP_HINT_DELAY_MS = 3500;
  let showForceStop = $state(false);
  $effect(() => {
    const stopping = (selected?.running ?? false) && action === 'stop';
    if (!stopping) {
      showForceStop = false;
      return;
    }
    const timer = setTimeout(() => {
      showForceStop = true;
    }, FORCE_STOP_HINT_DELAY_MS);
    return () => clearTimeout(timer);
  });

  // Which add-on folder this server exposes: plugin cores (Paper/Purpur) keep
  // jars in runtime/plugins, mod cores (Fabric/Quilt/Forge/NeoForge) in
  // runtime/mods. Vanilla has neither — the button stays hidden there.
  const addonKind = $derived.by((): 'mods' | 'plugins' | null => {
    if (!selected) return null;
    if (pluginCapable(selected.loader)) return 'plugins';
    if (modCapable(selected.loader)) return 'mods';
    return null;
  });

  // §7 fallback gating: creating a server while the data root is unavailable
  // would write it into the wrong (temporary default) root. See
  // data-root-gating.ts / the data-root doc.
  const createDisabledReason = $derived.by(() => {
    const key = dataRootCreateDisabledKey(dataLocation.fellBack);
    return key === null ? null : $t(key);
  });

  function statusLabelFor(s: ServerWithStatus): string | null {
    const kind = serverNavStatus(s);
    if (kind === 'running') return $t('servers.status.running');
    if (kind === 'crashed' || kind === 'fixable') return $t('servers.status.crashed');
    return null;
  }

  function openCreate(): void {
    serversUi.creating = true;
    // The wizard lives in the right panel, which compact mode unmounts —
    // mirror the wide-overlay auto-expand in +page.svelte.
    if (compactState.value) void setCompact(false);
  }

  async function startSelected(): Promise<void> {
    const id = selected?.id;
    if (!id) return;
    // Show Overview first so startup lines (and any EULA/port failure
    // banner) land on its console card — then run the shared helper.
    serversUi.activeTab = 'overview';
    await serverState.start(id);
  }

  async function stopSelected(): Promise<void> {
    const id = selected?.id;
    if (!id) return;
    await serverState.stop(id);
  }

  async function killSelected(): Promise<void> {
    const id = selected?.id;
    if (!id) return;
    await serverState.kill(id);
  }

  async function openAddonsFolder(): Promise<void> {
    const id = selected?.id;
    if (!id) return;
    const key = addonKind === 'plugins' ? 'servers.plugins.openFolder' : 'servers.mods.openFolder';
    const r =
      addonKind === 'plugins'
        ? await serverState.openPluginsFolder(id)
        : addonKind === 'mods'
          ? await serverState.openModsFolder(id)
          : null;
    if (r && !r.ok) {
      pushWarning(get(t)(key), [formatError(r.error as IpcError)]);
    }
  }
</script>

{#snippet serverLeading(opt: SelectOption)}
  {@const s = serverState.list.find((x) => x.id === opt.value)}
  {#if s}
    <NavStatusIcon
      name="server"
      size={16}
      iconClass={navVisual(serverNavStatus(s)).iconClass}
      statusLabel={statusLabelFor(s)}
    />
  {/if}
{/snippet}

<div class="flex flex-col gap-1 pt-3 border-t border-border-subtle">
  <div class="text-xs uppercase tracking-wide text-muted">
    <span>{$t('sidebar.server')}</span>
  </div>

  {#if serverState.list.length > 0}
    <Select
      class="w-full text-sm"
      value={serversUi.selectedServerId ?? ''}
      options={serverOptions}
      onChange={(v) => serversUi.selectServer(String(v))}
      ariaLabel={$t('sidebar.server')}
      optionLeading={serverLeading}
      valueLeading={serverLeading}
      dataTestid="sidebar-server-select"
    />
  {:else if serverState.listLoadedOnce}
    <!-- Only claim "no servers" after a settled load — while the initial fetch
         is still in flight show nothing (the label + create button remain). -->
    <p class="text-xs text-muted">{$t('sidebar.noServers')}</p>
  {/if}

  <!-- Always-present span wrapper (tooltip param null when enabled) so the
       tour anchor + testid stay on the button in both states. -->
  <span
    class="inline-flex w-full"
    use:tooltip={createDisabledReason ? { text: createDisabledReason, describe: false } : null}
  >
    <button
      type="button"
      class="btn-secondary btn-xs w-full flex items-center justify-center gap-1"
      data-tour-ctx="servers-create"
      data-testid="sidebar-create-server"
      disabled={createDisabledReason !== null}
      onclick={openCreate}
    >
      <Icon name="plus" size={14} />
      {$t('servers.create')}
    </button>
  </span>

  {#if selected}
    {#if addonKind}
      <button
        type="button"
        class="btn-secondary btn-xs w-full flex items-center justify-center gap-1"
        data-testid="sidebar-server-addons-folder"
        onclick={() => void openAddonsFolder()}
      >
        <Icon name="folderOpen" size={14} />
        {addonKind === 'plugins' ? $t('sidebar.plugins') : $t('sidebar.mods')}
      </button>
    {/if}

    {#if selected.running}
      <BusyButton
        class="btn-danger btn-lg flex items-center justify-center"
        busy={action === 'stop'}
        disabled={action !== null && action !== 'stop'}
        data-testid="sidebar-server-stop"
        onclick={() => void stopSelected()}
      >
        <Icon name="stop" size={16} />
        {$t('servers.action.stop')}
      </BusyButton>
      {#if showForceStop}
        <!-- Escalation out of a stuck graceful stop: skip the wait, hard-kill now.
             Surfaces only after FORCE_STOP_HINT_DELAY_MS so a normal stop is clean. -->
        <BusyButton
          class="btn-ghost-danger flex items-center justify-center"
          busy={killing}
          data-testid="sidebar-server-force-stop"
          onclick={() => void killSelected()}
        >
          {$t('servers.action.forceStop')}
        </BusyButton>
      {/if}
    {:else}
      <span
        class="inline-flex w-full"
        use:tooltip={uploading
          ? { text: $t('servers.hosting.startBlockedByUpload'), describe: false }
          : null}
      >
        <BusyButton
          class="btn-success btn-lg w-full flex items-center justify-center"
          busy={action === 'start'}
          disabled={uploading || (action !== null && action !== 'start')}
          data-testid="sidebar-server-start"
          onclick={() => void startSelected()}
        >
          <Icon name="play" size={16} />
          {$t('servers.action.start')}
        </BusyButton>
      </span>
    {/if}
  {/if}
</div>

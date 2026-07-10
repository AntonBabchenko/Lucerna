<script lang="ts">
  import { onMount } from 'svelte';
  import { serverNavStatus, serverState } from '$lib/servers/server-state.svelte';
  import { Icon } from '$lib/ui/icons';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { displayCore } from '$lib/servers/core-display';
  import { navVisual, type NavStatusKind } from '$lib/layout/nav-status';
  import NavStatusIcon from '$lib/layout/NavStatusIcon.svelte';
  import CardShell from '$lib/ui/cards/CardShell.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import type { CardAccent, BadgeVariant } from '$lib/ui/cards/card-status';
  import type { InstanceWithStatus, ServerWithStatus, VersionEntry } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/ipc/format-error';
  import ServerCreateWizard from './ServerCreateWizard.svelte';
  import ServerManageView from './ServerManageView.svelte';
  import DeleteServerDialog from './DeleteServerDialog.svelte';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { SERVERS_STEPS } from '$lib/onboarding/contextual-tours';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import { dataRootCreateDisabledKey } from '$lib/settings/data-root-gating';

  let {
    instances,
    versions,
    onInstanceCreated,
  }: {
    instances: InstanceWithStatus[];
    versions: VersionEntry[];
    onInstanceCreated: (instanceId: string) => void;
  } = $props();
  let selected = $state<string | null>(null);
  let creating = $state(false);
  let pendingDelete = $state<{ id: string; name: string } | null>(null);
  let deleteError = $state<string | null>(null);

  onMount(() => {
    serverState.init();
    void serverState.refresh();
  });

  async function confirmDelete() {
    deleteError = null;
    const target = pendingDelete;
    if (!target) return;
    pendingDelete = null;
    const r = await serverState.remove(target.id);
    if (!r.ok) deleteError = formatError(r.error as Parameters<typeof formatError>[0]);
  }

  // Row presentation derived from the per-server status. The icon (NavStatusIcon)
  // reuses the sidebar's colour/pulse via navVisual(); the CardShell accent strip
  // and the textual StatusBadge variant come from the same kind so the row's
  // colour is never the only signal. Every state has a non-null label so the
  // icon's colour always has an accessible text alternative (idle included).
  const STATUS_ACCENT: Record<NavStatusKind, CardAccent> = {
    running: 'success',
    crashed: 'danger',
    fixable: 'warning',
    idle: 'none',
    advisory: 'warning',
    actionable: 'warning',
  };
  const STATUS_BADGE: Record<NavStatusKind, BadgeVariant> = {
    running: 'success',
    crashed: 'danger',
    fixable: 'warning',
    idle: 'muted',
    advisory: 'warning',
    actionable: 'warning',
  };

  function statusLabel(s: ServerWithStatus): string {
    const kind = serverNavStatus(s);
    if (kind === 'running') return $t('sidebar.serverRunning');
    if (kind === 'crashed') return $t('sidebar.serverCrashed');
    if (kind === 'fixable') return $t('sidebar.serversFixAvailable');
    return $t('servers.status.stopped');
  }

  // The compact textual state word for the StatusBadge (running/crashed/stopped).
  // Fixable reuses the "fix available" sidebar phrasing so the badge text matches
  // the amber tone; other states use the existing servers.status.* vocabulary.
  function statusText(s: ServerWithStatus): string {
    const kind = serverNavStatus(s);
    if (kind === 'running') return $t('servers.status.running');
    if (kind === 'crashed') return $t('servers.status.crashed');
    if (kind === 'fixable') return $t('sidebar.serversFixAvailable');
    return $t('servers.status.stopped');
  }

  // §7 fallback gating: creating a server while the data root is unavailable
  // would write it into the wrong (temporary default) root. See
  // data-root-gating.ts / the data-root design doc.
  const createDisabledReason = $derived.by(() => {
    const key = dataRootCreateDisabledKey(dataLocation.fellBack);
    return key === null ? null : $t(key);
  });

  // Resolve a server's `created_from_instance` (a raw internal instance id) to
  // the source instance's display name for the row subtitle. Mirrors the
  // symmetric provenance line in ManageInstancesModal (created_from_server →
  // server name). A since-deleted source instance is absent from the map, so
  // the caller omits the segment rather than showing a meaningless id.
  const instanceNameById = $derived(new Map(instances.map((i) => [i.id, i.name])));
</script>

{#if creating}
  <ServerCreateWizard
    {instances}
    {versions}
    onDone={() => {
      creating = false;
      void serverState.refresh();
    }}
    onCancel={() => (creating = false)}
  />
{:else if selected}
  {#key selected}
    <ServerManageView serverId={selected} onBack={() => (selected = null)} {onInstanceCreated} />
  {/key}
{:else}
  <div class="flex flex-col gap-3 p-4 overflow-y-auto">
    <!-- No list heading here — ServersModal already renders the "Servers" title
         (h2#servers-modal-title); a second one inside the list was redundant. -->
    <div class="flex items-center justify-end">
      {#if createDisabledReason}
        <span class="inline-flex" use:tooltip={{ text: createDisabledReason, describe: false }}>
          <button
            type="button"
            data-tour-ctx="servers-create"
            class="btn-primary btn-sm flex items-center gap-1.5"
            disabled
          >
            <Icon name="plus" size={16} />
            {$t('servers.create')}
          </button>
        </span>
      {:else}
        <button
          type="button"
          data-tour-ctx="servers-create"
          class="btn-primary btn-sm flex items-center gap-1.5"
          onclick={() => (creating = true)}
        >
          <Icon name="plus" size={16} />
          {$t('servers.create')}
        </button>
      {/if}
    </div>
    <div data-tour-ctx="servers-list">
      {#if serverState.listLoading && serverState.list.length === 0}
        <!-- First load: a real spinner, not a misleading "no servers yet". -->
        <div class="flex justify-center py-8 text-secondary" data-testid="servers-list-loading">
          <Spinner labelPlacement="below" label={$t('servers.loading')} />
        </div>
      {:else if serverState.listError && serverState.list.length === 0}
        <!-- Fetch failed: distinct error + retry, not the empty state. -->
        <div
          class="flex flex-col items-center gap-2 py-8 text-center"
          data-testid="servers-list-error"
          role="alert"
        >
          <p class="text-sm text-danger">{$t('servers.loadError')}</p>
          <p class="text-xs text-muted break-all">
            {formatError(serverState.listError as Parameters<typeof formatError>[0])}
          </p>
          <button
            type="button"
            class="btn-secondary btn-sm"
            onclick={() => void serverState.refresh()}
          >
            {$t('servers.retry')}
          </button>
        </div>
      {:else if serverState.list.length === 0}
        <p class="text-muted text-sm">{$t('servers.empty')}</p>
      {:else}
        <!-- One bordered/rounded container holds the whole list; each CardShell
           row carries its own border-b separator (the canonical row-variant
           container pattern, matching InstalledAssetsView). -->
        <div class="overflow-hidden rounded-lg border border-border-subtle">
          {#each serverState.list as s (s.id)}
            {@const navKind = serverNavStatus(s)}
            {@const sourceName = s.created_from_instance
              ? instanceNameById.get(s.created_from_instance)
              : undefined}
            <CardShell variant="row" accent={STATUS_ACCENT[navKind]}>
              <button
                type="button"
                class="flex flex-1 items-center gap-3 text-left"
                onclick={() => (selected = s.id)}
              >
                <!-- Same colour + green saturation pulse / red / amber / neutral as the
                   sidebar "Servers" icon, with a text alternative so status is never
                   colour-only (DESIGN.md §13). -->
                <NavStatusIcon
                  name="server"
                  size={20}
                  iconClass={navVisual(navKind).iconClass}
                  statusLabel={statusLabel(s)}
                />
                <span class="flex-1">
                  <span class="block font-medium">{s.name}</span>
                  <span class="block text-xs text-muted"
                    >{s.mc_version} · {displayCore(s.loader)}{sourceName
                      ? ' · ' + $t('servers.createdFromInstance', { name: sourceName })
                      : ''}</span
                  >
                </span>
                <StatusBadge variant={STATUS_BADGE[navKind]}>
                  {statusText(s)}{s.port ? ' · ' + s.port : ''}
                </StatusBadge>
                <Icon name="arrowRight" size={16} />
              </button>
              <!-- Wrapper span carries the tooltip so the "why disabled" hint still
                 shows while the button is disabled (disabled elements swallow
                 pointer events). aria-label stays the accessible name; the tooltip
                 is describe:false so it isn't double-announced. -->
              <span
                class="inline-flex shrink-0"
                use:tooltip={{
                  text: s.running
                    ? $t('servers.delete.runningBlock')
                    : $t('servers.delete.trigger'),
                  describe: false,
                }}
              >
                <button
                  type="button"
                  class="btn-icon btn-icon-sm btn-icon-danger"
                  aria-label={$t('servers.delete.trigger')}
                  disabled={s.running}
                  onclick={() => (pendingDelete = { id: s.id, name: s.name })}
                >
                  <Icon name="trash" size={16} />
                </button>
              </span>
            </CardShell>
          {/each}
        </div>
      {/if}
    </div>
    <p
      class="text-xs text-muted border border-dashed border-border-subtle rounded-lg p-3"
      data-tour-ctx="servers-lan"
    >
      {$t('servers.lanHint')}
    </p>
    {#if deleteError}
      <p class="text-sm text-danger" role="alert">{deleteError}</p>
    {/if}
  </div>
  {#if pendingDelete}
    <DeleteServerDialog
      serverName={pendingDelete.name}
      onCancel={() => (pendingDelete = null)}
      onConfirm={() => void confirmDelete()}
    />
  {/if}
  <ContextualTour id="servers" steps={SERVERS_STEPS} />
{/if}

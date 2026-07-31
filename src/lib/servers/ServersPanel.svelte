<script lang="ts">
  // Full-panel servers UI for the Servers mode: the right-hand mirror of the
  // instance panel. Owns the loading / error / wizard / empty-hero / manage
  // branches; the selected server and active tab live in the shared serversUi
  // module (written by the sidebar, read here), and Start/Stop moved to the
  // sidebar — the header keeps only Restart.
  import {
    type InstanceWithStatus,
    type ServerWithStatus_Serialize,
    type VersionEntry,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { serversUi, type ServerTab } from '$lib/servers/servers-ui.svelte';
  import { Icon } from '$lib/ui/icons';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import DiagnosisRestoreButton from '$lib/ui/DiagnosisRestoreButton.svelte';
  import { diagnosisDismiss } from '$lib/ui/diagnosis-dismiss.svelte';
  import { serverBannerEligible, serverDiagnosisSignature } from './server-diagnosis-view';
  import ServerOverviewTab from '$lib/servers/overview/ServerOverviewTab.svelte';
  import ServerSettingsTab from '$lib/servers/settings/ServerSettingsTab.svelte';
  import ServerAddonsTab from '$lib/servers/addons/ServerAddonsTab.svelte';
  import ServerDiagnosisBanner from './ServerDiagnosisBanner.svelte';
  import ServerHostingTab from './ServerHostingTab.svelte';
  import ServerToInstanceDialog from './ServerToInstanceDialog.svelte';
  import ServerBackupsView from './ServerBackupsView.svelte';
  import ServerCreateWizard from './ServerCreateWizard.svelte';
  import { isCrashed } from './runtime-extra';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { SERVER_MANAGE_STEPS, SERVERS_STEPS } from '$lib/onboarding/contextual-tours';
  import { tooltip } from '$lib/ui/tooltip';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import { dataRootCreateDisabledKey } from '$lib/settings/data-root-gating';

  let {
    visible,
    instances,
    versions,
    onInstanceCreated,
  }: {
    // Whether the servers mode is the active panel. The panel stays mounted
    // in client mode (hidden) so console scroll / wizard progress survive —
    // contextual tours must not fire while hidden, hence this prop.
    visible: boolean;
    instances: InstanceWithStatus[];
    versions: VersionEntry[];
    onInstanceCreated: (instanceId: string) => void;
  } = $props();

  const serverId = $derived(serversUi.selectedServerId);
  // serverList() always returns ServerWithStatus_Serialize[]; the store type
  // is the union for legacy reasons. Cast here so the dialog prop is satisfied.
  const server = $derived(
    serverState.list.find((s) => s.id === serverId) as ServerWithStatus_Serialize | undefined,
  );
  const running = $derived(serverId !== null && serverState.running(serverId));
  const uploading = $derived(serverId !== null && serverState.isUploading(serverId));
  // #18: distinguish a crash from a clean stop in the header pill (C1 shim).
  const crashed = $derived(server ? isCrashed(server) : false);

  // Restore affordance for a dismissed diagnosis banner: shown only when the
  // banner would otherwise be visible but the user hid it (mirror of the
  // overview's attention-restore badge).
  const diagnosis = $derived(serverId !== null ? serverState.diagnosisFor(serverId) : undefined);
  const diagSignature = $derived(serverDiagnosisSignature(diagnosis));
  const showDiagnosisRestore = $derived(
    serverId !== null &&
      serverBannerEligible(diagnosis, running) &&
      diagSignature !== null &&
      diagnosisDismiss.isDismissed(`server:${serverId}`, diagSignature),
  );

  // Lifecycle busy/error state lives in the store (shared with the sidebar's
  // Start/Stop) — a start failure triggered from the sidebar surfaces here.
  const actionError = $derived(server ? serverState.actionErrorFor(server.id) : undefined);

  // The store records action AND list errors RAW: either a typed IPC Result
  // error (object with a `kind`) or a thrown value (transport failure).
  // formatError only understands the former — JSON.stringify(new Error())
  // renders as "{}" — so route by shape (same split as
  // ServerToInstanceDialog's create()).
  function describeStoreError(e: unknown): string {
    if (typeof e === 'object' && e !== null && typeof (e as { kind?: unknown }).kind === 'string') {
      return formatError(e as Parameters<typeof formatError>[0]);
    }
    return e instanceof Error ? e.message : String(e);
  }

  // Which server the to-instance dialog is open FOR (null = closed). Captured
  // at open and compared against the current selection in the render gate, so
  // a selection change can never show the dialog against the wrong server —
  // not even for the one frame before the selection-change effect runs.
  let toInstanceFor = $state<string | null>(null);

  // §7 fallback gating: creating anything (a server via the hero create, or a
  // client instance from this server) would write it into the wrong
  // (temporary default) root while the configured data root is unavailable.
  // One derived, read by both create entry points. See data-root-gating.ts.
  const createDisabledReason = $derived.by(() => {
    const key = dataRootCreateDisabledKey(dataLocation.fellBack);
    return key === null ? null : $t(key);
  });

  // WAI-ARIA tabs pattern: roving tabindex with arrow-key navigation.
  const TAB_ORDER: ServerTab[] = ['overview', 'settings', 'addons', 'hosting', 'backups'];
  let tabEls = $state<(HTMLButtonElement | null)[]>([]);

  function onTablistKeydown(e: KeyboardEvent) {
    const current = TAB_ORDER.indexOf(serversUi.activeTab);
    if (current === -1) return;
    let next = current;
    if (e.key === 'ArrowRight') next = (current + 1) % TAB_ORDER.length;
    else if (e.key === 'ArrowLeft') next = (current - 1 + TAB_ORDER.length) % TAB_ORDER.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = TAB_ORDER.length - 1;
    else return;
    e.preventDefault();
    serversUi.activeTab = TAB_ORDER[next];
    tabEls[next]?.focus();
  }

  // Diagnose whenever the SELECTION changes (first mount included). The
  // {#key server.id} below remounts only the keyed markup, not this
  // component, so a plain onMount-diagnose would fire once for the first
  // selection only. Plain (non-$state) bookkeeping: only this effect reads
  // it, so it must not be a dependency.
  let prevId: string | null = null;
  $effect(() => {
    const id = serverId;
    if (id !== null && id !== prevId) {
      void serverState.diagnose(id);
      // The render gate already hides a dialog opened against the previous
      // selection (mismatched id); clearing here also keeps it from
      // REAPPEARING if the user re-selects that server later.
      toInstanceFor = null;
    }
    prevId = id;
  });

  // Re-diagnose when the server stops (running → false transition). Previous
  // value in plain variables so the $effect reads both old and new; scoped to
  // the SAME server so switching from a running server to a stopped one isn't
  // mistaken for a stop.
  let prevRunning = false;
  let prevRunningId: string | null = null;
  $effect(() => {
    const id = serverId;
    const isRunning = id !== null && serverState.running(id);
    if (id !== null && id === prevRunningId && prevRunning && !isRunning) {
      void serverState.diagnose(id);
    }
    prevRunning = isRunning;
    prevRunningId = id;
  });
</script>

<div class="flex flex-col overflow-hidden h-full">
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
      <!-- listError holds a typed IPC error OR a thrown transport Error (see
           refresh() in server-state) — route by shape like action errors. -->
      <p class="text-xs text-muted break-all">
        {describeStoreError(serverState.listError)}
      </p>
      <button type="button" class="btn-secondary btn-sm" onclick={() => void serverState.refresh()}>
        {$t('servers.retry')}
      </button>
    </div>
  {:else if serversUi.creating}
    <!-- The wizard scrolls inside the panel: its own overflow-y-auto needs a
         bounded height, which this flex-1 + min-h-0 wrapper provides. -->
    <div class="flex-1 min-h-0 overflow-y-auto">
      <ServerCreateWizard
        {instances}
        {versions}
        onDone={(createdId) => {
          serversUi.creating = false;
          if (createdId) serversUi.selectServer(createdId);
        }}
        onCancel={() => (serversUi.creating = false)}
      />
    </div>
  {:else if !server}
    <div
      class="flex flex-col items-center justify-center gap-3 py-16 text-center"
      data-testid="servers-empty-hero"
    >
      <Icon name="server" size={40} class="text-muted" />
      <p class="font-medium text-primary">{$t('servers.empty')}</p>
      <p class="text-sm text-muted max-w-md">{$t('servers.lanHint')}</p>
      <!-- Always-present span wrapper (empty tooltip no-ops when enabled) so
           the testid stays on the button in both states. -->
      <span class="inline-flex" use:tooltip={{ text: createDisabledReason ?? '', describe: false }}>
        <button
          type="button"
          class="btn-primary btn-sm flex items-center gap-1.5"
          data-testid="servers-hero-create"
          disabled={createDisabledReason !== null}
          onclick={() => (serversUi.creating = true)}
        >
          <Icon name="plus" size={14} />
          {$t('servers.create')}
        </button>
      </span>
    </div>
  {:else}
    {#key server.id}
      <!-- Header -->
      <div class="flex items-center gap-3 border-b border-border-subtle px-4 py-2">
        <span class="flex-1 font-semibold truncate">{server.name}</span>

        <!-- Restore a dismissed diagnosis banner -->
        {#if showDiagnosisRestore}
          <DiagnosisRestoreButton
            testid="server-diagnosis-restore"
            onRestore={() => diagnosisDismiss.restore(`server:${server.id}`)}
          />
        {/if}

        <!-- Status pill -->
        <StatusBadge variant={running ? 'success' : crashed ? 'danger' : 'muted'}>
          {running
            ? $t('servers.status.running')
            : crashed
              ? $t('servers.status.crashed')
              : $t('servers.status.stopped')}{server.port ? ' · ' + server.port : ''}
        </StatusBadge>

        <!-- Actions. Start/Stop live in the sidebar (the mirror of the client
             Play button, and where the tour's start/stop step points); Restart
             stays here as a manage-surface action. -->
        <div class="flex items-center gap-1.5">
          <span
            class="inline-flex"
            use:tooltip={{ text: createDisabledReason ?? '', describe: false }}
          >
            <button
              type="button"
              class="btn-ghost btn-sm flex items-center gap-1"
              onclick={() => (toInstanceFor = server.id)}
              disabled={createDisabledReason !== null}
              data-testid="create-client-instance-btn"
              data-tour-ctx="server-to-instance"
            >
              <Icon name="download" size={14} />
              {$t('servers.toInstance.button')}
            </button>
          </span>
          {#if running}
            <BusyButton
              class="btn-secondary btn-sm"
              busy={serverState.actionFor(server.id) === 'restart'}
              disabled={uploading ||
                (serverState.actionFor(server.id) !== null &&
                  serverState.actionFor(server.id) !== 'restart')}
              onclick={() => void serverState.restart(server.id)}
            >
              <Icon name="refresh" size={14} />{$t('servers.action.restart')}
            </BusyButton>
          {/if}
          {#if uploading}
            <span class="text-xs text-muted">{$t('servers.hosting.startBlockedByUpload')}</span>
          {/if}
        </div>
      </div>

      <!-- Inline action error is the UNCLASSIFIED fallback only: when a failed
           start's diagnose() produced a rich banner for this server, the banner
           owns the message and we suppress this duplicate. -->
      {#if actionError !== undefined && !serverState.diagnosisFor(server.id)}
        <p class="px-4 pt-2 text-sm text-danger" role="alert" data-testid="server-action-error">
          {describeStoreError(actionError)}
        </p>
      {/if}

      <!-- Diagnosis banner (shown when the server crash-diagnosed after stop) -->
      <div class="px-4 pt-2">
        <ServerDiagnosisBanner serverId={server.id} />
      </div>

      <!-- Sub-tabs. overflow-x-auto is load-bearing: at the 820px window minimum
           the RU tab labels overflow the panel. overflow-y-hidden is too:
           overflow-x:auto forces overflow-y to compute to auto, and the tabs'
           -mb-px underline overlap overflows 1px vertically — without the
           explicit hidden that 1px grows a stray vertical scrollbar. -->
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div
        role="tablist"
        class="flex gap-1 overflow-x-auto overflow-y-hidden border-b border-border-subtle px-4 bg-surface"
        onkeydown={onTablistKeydown}
      >
        {#each [['overview', $t('servers.tab.overview')], ['settings', $t('servers.tab.settings')], ['addons', $t('servers.tab.addons')], ['hosting', $t('servers.tab.hosting')], ['backups', $t('servers.tab.backups')]] as const as [id, label] (id)}
          <button
            bind:this={tabEls[TAB_ORDER.indexOf(id)]}
            type="button"
            role="tab"
            data-tour-ctx={`server-tab-${id}`}
            aria-selected={serversUi.activeTab === id}
            tabindex={serversUi.activeTab === id ? 0 : -1}
            class="px-3 py-2 text-sm border-b-2 -mb-px transition-colors"
            class:border-accent={serversUi.activeTab === id}
            class:text-primary={serversUi.activeTab === id}
            class:font-semibold={serversUi.activeTab === id}
            class:border-transparent={serversUi.activeTab !== id}
            class:text-muted={serversUi.activeTab !== id}
            onclick={() => (serversUi.activeTab = id)}
          >
            {label}
          </button>
        {/each}
      </div>

      <!-- Tab body -->
      <div class="flex-1 overflow-y-auto p-4">
        {#if serversUi.activeTab === 'overview'}
          <ServerOverviewTab serverId={server.id} />
        {:else if serversUi.activeTab === 'settings'}
          <ServerSettingsTab serverId={server.id} />
        {:else if serversUi.activeTab === 'addons'}
          <ServerAddonsTab serverId={server.id} />
        {:else if serversUi.activeTab === 'hosting'}
          <ServerHostingTab serverId={server.id} />
        {:else if serversUi.activeTab === 'backups'}
          <ServerBackupsView serverId={server.id} />
        {/if}
      </div>
    {/key}
  {/if}
</div>

<!-- One-shot tours, gated on this being the ACTIVE panel: the panel stays
     mounted (hidden) in client mode, and a tour must not fire off-screen. The
     wizard suppresses both (its own flow is self-explanatory and the anchors
     are hidden); the list tour additionally waits until the FIRST list fetch
     has settled — before that, "no server" just means "not loaded yet", and a
     tour mounted in that window would be unmounted by the spinner branch and
     burned as soft-skipped (ContextualTour marks itself seen on destroy). -->
{#if visible && !serversUi.creating}
  {#if server}
    <ContextualTour id="serverManage" steps={SERVER_MANAGE_STEPS} />
  {:else if serverState.listLoadedOnce}
    <ContextualTour id="servers" steps={SERVERS_STEPS} />
  {/if}
{/if}

{#if server && toInstanceFor === server.id}
  <ServerToInstanceDialog
    {server}
    onCancel={() => (toInstanceFor = null)}
    onCreated={(id) => {
      toInstanceFor = null;
      onInstanceCreated(id);
    }}
  />
{/if}

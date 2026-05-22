<script lang="ts">
  import {
    commands,
    events,
    type Account,
    type CrashReport,
    type Error as IpcError,
    type InstanceWithStatus,
    type MissingModStatus,
    type VersionEntry,
  } from '$lib/ipc/bindings';
  import NetworkPopover from '$lib/network/NetworkPopover.svelte';
  import PhaseStatusRow from '$lib/install/PhaseStatusRow.svelte';
  import LogsPopover from '$lib/logs/LogsPopover.svelte';
  import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';
  import SettingsModal from '$lib/settings/SettingsModal.svelte';
  import Sidebar from '$lib/layout/Sidebar.svelte';
  import MainTabs from '$lib/layout/MainTabs.svelte';
  import TourOverlay from '$lib/onboarding/TourOverlay.svelte';
  import ToastHost from '$lib/toasts/ToastHost.svelte';
  import { initOnboarding } from '$lib/onboarding/state.svelte';
  import { onMount, untrack } from 'svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { formatError } from '$lib/ipc/format-error';
  import { modBrowserNav, modpacksNav } from '$lib/settings/state.svelte';

  let accounts = $state<Account[]>([]);
  let activeAccount = $state<Account | null>(null);
  let offlineNameError = $state<string | null>(null);
  let listAccountsError = $state<string | null>(null);
  let removeError = $state<string | null>(null);
  let networkOpen = $state(false);

  // Vanilla MC version manifest, still fetched on mount and passed
  // through to ManageInstancesModal where it powers the MC version
  // picker. Home no longer renders a version dropdown — see Manage.
  let versions = $state<VersionEntry[]>([]);
  let versionsError = $state<string | null>(null);

  let instances = $state<InstanceWithStatus[]>([]);
  let activeInstance = $state<InstanceWithStatus | null>(null);
  let instancesError = $state<string | null>(null);

  let manageOpen = $state(false);

  // Lightweight installed-mods stats for the Overview pane. Re-fetched
  // on instance change and whenever the launcher emits an install /
  // uninstall / toggle event from the mod browser.
  let installedStats = $state<{ total: number; enabled: number; disabled: number }>({
    total: 0,
    enabled: 0,
    disabled: 0,
  });

  async function refreshInstalledStats(id: string | null) {
    if (!id) {
      installedStats = { total: 0, enabled: 0, disabled: 0 };
      return;
    }
    const r = await commands.modsListInstalled(id);
    if (r.status !== 'ok') return;
    const total = r.data.length;
    const enabled = r.data.filter((m) => m.enabled).length;
    installedStats = { total, enabled, disabled: total - enabled };
  }

  // Missing mods for the active pack-origin instance — drives the
  // Overview indicator. Empty for non-pack instances and pre-SF2
  // imports (modpack_status returns null or an empty list).
  let packMissingMods = $state<MissingModStatus[]>([]);
  const unresolvedMissing = $derived(packMissingMods.filter((m) => m.state !== 'installed'));

  async function refreshPackStatus(id: string | null) {
    if (!id) {
      packMissingMods = [];
      return;
    }
    const r = await commands.modpackStatus(id);
    packMissingMods = r.status === 'ok' && r.data ? r.data.missing_mods : [];
  }

  let installing = $state(false);
  let installError = $state<string | null>(null);
  let running = $state<{ pid: number; version_id: string } | null>(null);
  let exited = $state<{ code: number; log_path: string } | null>(null);
  let spawnUnlisten: (() => void) | null = null;
  let exitUnlisten: (() => void) | null = null;

  let logsOpen = $state(false);
  let logsInitialPath = $state<string | null>(null);
  let crashReport = $state<CrashReport | null>(null);
  let modsError = $state<string | null>(null);
  let violationsCount = $state(0);

  // Whenever the active instance changes, clear per-instance error banners.
  // They refer to the previously-active instance and confuse the user when
  // they switch context (e.g. fix one instance's setup by switching to
  // another, only to still see the old error).
  let lastActiveId: string | null = null;
  $effect(() => {
    const newId = activeInstance?.id ?? null;
    untrack(() => {
      if (newId !== lastActiveId) {
        lastActiveId = newId;
        installError = null;
        modsError = null;
        exited = null;
        crashReport = null;
        void refreshInstalledStats(newId);
        void refreshPackStatus(newId);
      }
    });
  });

  async function refreshAccounts() {
    const list = await commands.listAccounts();
    if (list.status === 'ok') {
      accounts = list.data;
    } else {
      listAccountsError = formatError(list.error);
    }
    const active = await commands.getActiveAccount();
    if (active.status === 'ok') {
      activeAccount = active.data;
    }
  }

  onMount(async () => {
    void refreshInstances();

    events.processSpawned
      .listen((event) => {
        running = { pid: event.payload.pid, version_id: event.payload.version_id };
        exited = null;
      })
      .then((u) => {
        spawnUnlisten = u;
      });

    // Mod-install events refresh the Overview stats so the user can
    // see the Total / Enabled / Disabled numbers tick up after install
    // from the Mod browser without bouncing back through this view.
    events.modInstalled.listen(() => {
      void refreshInstalledStats(activeInstance?.id ?? null);
      void refreshPackStatus(activeInstance?.id ?? null);
    });
    events.modUninstalled.listen(() => {
      void refreshInstalledStats(activeInstance?.id ?? null);
      void refreshPackStatus(activeInstance?.id ?? null);
    });
    events.modToggle.listen(() => refreshInstalledStats(activeInstance?.id ?? null));

    events.processExited
      .listen(async (event) => {
        running = null;
        exited = { code: event.payload.code, log_path: event.payload.log_path };
        void refreshInstances();
        if (event.payload.code !== 0 && activeInstance) {
          const result = await commands.latestCrash(activeInstance.id);
          if (result.status === 'ok' && result.data) {
            crashReport = result.data;
          }
        } else {
          crashReport = null;
        }
        await refreshViolations();
      })
      .then((u) => {
        exitUnlisten = u;
      });

    await refreshViolations();

    await refreshAccounts();

    const versionsResult = await commands.listVersions();
    if (versionsResult.status === 'ok') {
      versions = versionsResult.data;
    } else {
      versionsError = formatError(versionsResult.error);
    }

    void initOnboarding();
  });

  async function onSelectAccount(id: string) {
    const result = await commands.setActiveAccount(id);
    if (result.status === 'ok') {
      await refreshAccounts();
    }
  }

  async function onRemoveActive() {
    if (!activeAccount) return;
    removeError = null;
    const result = await commands.removeAccount(activeAccount.id);
    if (result.status === 'ok') {
      await refreshAccounts();
    } else {
      removeError = formatError(result.error);
    }
  }

  async function refreshInstances() {
    instancesError = null;
    const list = await commands.listInstances();
    if (list.status === 'ok') {
      instances = list.data;
    } else {
      instancesError = formatError(list.error);
      instances = [];
    }
    const active = await commands.getActiveInstance();
    if (active.status === 'ok') {
      activeInstance = active.data;
    } else {
      activeInstance = null;
    }
  }

  async function onSelectInstance(id: string) {
    const result = await commands.setActiveInstance(id);
    if (result.status === 'error') {
      instancesError = formatError(result.error);
      return;
    }
    await refreshInstances();
    // The $effect watching activeInstance.id clears per-instance error
    // banners (installError, modsError, exited, crashReport) automatically.
  }

  async function onInstall() {
    if (!activeInstance) return;
    if (activeInstance.mc_version === '') return;
    installing = true;
    installError = null;
    const result = await commands.installInstance(activeInstance.id);
    installing = false;
    if (result.status === 'error') {
      installError = formatError(result.error);
    } else {
      // Refresh so `activeInstance.ready` flips to true and the button
      // swaps from blue Install to green Play. No auto-launch — the
      // user clicks Play explicitly.
      await refreshInstances();
    }
  }

  async function onPlay() {
    if (!activeInstance) return;
    if (activeInstance.mc_version === '') return;
    if (!activeInstance.ready) return;
    installError = null;
    const result = await commands.launchInstance(activeInstance.id);
    if (result.status === 'error') {
      installError = formatError(result.error);
    }
    // processSpawned event sets `running` once MC starts; processExited
    // clears it. No need to refresh state here.
  }

  async function onStop() {
    const result = await commands.stopMinecraft();
    if (result.status === 'error') {
      installError = formatError(result.error);
    }
  }

  function openCrashInLogs() {
    if (!crashReport) return;
    logsInitialPath = crashReport.path;
    logsOpen = true;
  }

  async function onOpenMods() {
    modsError = null;
    if (!activeInstance) return;
    const result = await commands.openModsFolder(activeInstance.id);
    if (result.status === 'error') {
      modsError = formatError(result.error);
    }
  }

  async function refreshViolations() {
    const v = await commands.networkAuditViolations();
    if (Array.isArray(v)) {
      violationsCount = v.length;
    }
  }
</script>

<main
  class="grid h-screen overflow-hidden"
  style="grid-template-columns: 240px 1fr; grid-template-rows: 1fr auto;"
>
  <div class="col-start-1 row-start-1 overflow-hidden">
    <Sidebar
      {accounts}
      {activeAccount}
      {instances}
      {activeInstance}
      {violationsCount}
      {onSelectAccount}
      onRemoveAccount={onRemoveActive}
      onAddOffline={async (name) => {
        if (!name) {
          offlineNameError = 'Name cannot be empty';
          return;
        }
        offlineNameError = null;
        const result = await commands.addOfflineAccount(name);
        if (result.status === 'ok') {
          await commands.setActiveAccount(result.data.id);
          await refreshAccounts();
        } else {
          offlineNameError = formatError(result.error);
        }
      }}
      {onSelectInstance}
      onOpenManage={() => (manageOpen = true)}
      {onOpenMods}
      onOpenLogs={() => (logsOpen = !logsOpen)}
      onOpenNetwork={() => {
        networkOpen = !networkOpen;
        if (!networkOpen) void refreshViolations();
      }}
      {running}
      {installing}
      {onPlay}
      {onStop}
      {onInstall}
    />
  </div>

  <div class="col-start-2 row-start-1 overflow-hidden flex flex-col">
    {#if crashReport}
      <div
        class="bg-red-50 border-b border-red-200 text-red-800 px-4 py-2 flex items-center justify-between gap-3"
      >
        <span class="text-sm">
          Minecraft crashed.
          <span class="font-mono text-xs">{crashReport.path.split(/[\\/]/).pop()}</span>
        </span>
        <div class="flex items-center gap-2">
          <button
            class="text-xs bg-red-600 text-white rounded px-2 py-1 hover:bg-red-700"
            onclick={openCrashInLogs}
          >
            View crash report
          </button>
          <button
            class="text-xs border border-red-400 rounded px-2 py-1 hover:bg-red-100"
            onclick={() => (crashReport = null)}
          >
            Dismiss
          </button>
        </div>
      </div>
    {/if}

    <MainTabs
      instanceId={activeInstance?.id ?? null}
      mcVersion={activeInstance?.mc_version ?? null}
      loader={activeInstance?.loader ?? null}
      {instances}
      onSwitchInstance={(id) => {
        void onSelectInstance(id);
      }}
      onListChanged={() => {
        void refreshInstances();
      }}
    >
      {#snippet overview()}
        <div class="p-6 flex flex-col gap-4">
          {#if offlineNameError}
            <p class="text-xs text-red-700">
              {offlineNameError}
              <button
                class="text-neutral-500 hover:text-neutral-800"
                onclick={() => (offlineNameError = null)}
                aria-label="Dismiss">×</button
              >
            </p>
          {/if}
          {#if listAccountsError}
            <p class="text-xs text-red-700">
              {listAccountsError}
              <button
                class="text-neutral-500 hover:text-neutral-800"
                onclick={() => (listAccountsError = null)}>×</button
              >
            </p>
          {/if}
          {#if removeError}
            <p class="text-xs text-red-700">
              {removeError}
              <button
                class="text-neutral-500 hover:text-neutral-800"
                onclick={() => (removeError = null)}>×</button
              >
            </p>
          {/if}
          {#if instancesError}
            <p class="text-xs text-red-700">{instancesError}</p>
          {/if}
          {#if versionsError}
            <p class="text-xs text-red-700">{versionsError}</p>
          {/if}
          {#if activeInstance}
            <div class="flex flex-col gap-1">
              <div class="text-xs uppercase tracking-wide text-neutral-500">Configuration</div>
              <div class="text-sm grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1">
                <span class="text-neutral-500">Minecraft</span>
                <span class="font-mono">{activeInstance.mc_version || '(not set)'}</span>
                <span class="text-neutral-500">Loader</span>
                <span class="font-mono">
                  {displayLoader(activeInstance.loader)}{#if activeInstance.loader_version}
                    · {activeInstance.loader_version}
                  {/if}
                </span>
                <span class="text-neutral-500">Memory</span>
                <span class="font-mono">{activeInstance.max_heap_mb} MB</span>
              </div>
              <p class="text-xs text-neutral-500">
                Edit via
                <button
                  type="button"
                  class="underline hover:text-neutral-800"
                  onclick={() => (manageOpen = true)}>Manage</button
                >.
              </p>
            </div>

            <div class="flex flex-col gap-1">
              <div class="text-xs uppercase tracking-wide text-neutral-500">Installed mods</div>
              {#if installedStats.total === 0}
                <p class="text-sm text-neutral-500">
                  No mods installed yet. Open
                  <button
                    type="button"
                    class="underline hover:text-neutral-800"
                    onclick={() => (modBrowserNav.value = { view: 'browse' })}
                  >
                    Mod browser
                  </button>
                  to add some.
                </p>
              {:else}
                <div class="text-sm flex gap-3">
                  <span
                    >Total: <span class="font-medium text-neutral-700">{installedStats.total}</span
                    ></span
                  >
                  <span
                    >Enabled: <span class="font-medium text-green-700"
                      >{installedStats.enabled}</span
                    ></span
                  >
                  <span
                    >Disabled: <span class="font-medium text-neutral-700"
                      >{installedStats.disabled}</span
                    ></span
                  >
                </div>
                <p class="text-xs text-neutral-500">
                  Manage in
                  <button
                    type="button"
                    class="underline hover:text-neutral-800"
                    onclick={() => (modBrowserNav.value = { view: 'installed' })}
                  >
                    Installed
                  </button>
                  tab.
                </p>
              {/if}
            </div>

            {#if unresolvedMissing.length > 0}
              <button
                type="button"
                class="flex items-center gap-2 text-sm text-left rounded border border-amber-200 bg-amber-50 px-3 py-2 hover:bg-amber-100"
                onclick={() => {
                  if (activeInstance) {
                    modpacksNav.value = { openDrawerForInstance: activeInstance.id };
                  }
                }}
                data-testid="overview-missing-mods"
              >
                <span aria-hidden="true">⚠</span>
                <span class="flex-1">
                  {unresolvedMissing.length}
                  {unresolvedMissing.length === 1 ? 'pack mod needs' : 'pack mods need'} attention
                </span>
                <span class="text-xs text-amber-700 underline">View</span>
              </button>
            {/if}

            <div class="flex items-center gap-4 mt-2">
              {#if running}
                <span class="text-sm font-mono"
                  >Running {running.version_id} (PID {running.pid})</span
                >
              {:else if activeInstance.mc_version === ''}
                <span class="text-sm text-neutral-500"
                  >Pick a Minecraft version in <button
                    type="button"
                    class="underline hover:text-neutral-800"
                    onclick={() => (manageOpen = true)}>Manage</button
                  > before installing.</span
                >
              {:else if installing}
                <span class="text-sm text-blue-700">Working…</span>
              {:else if !activeInstance.ready}
                <span class="text-sm text-neutral-500"
                  >Click <span class="font-semibold text-neutral-700">Install</span> in the sidebar to
                  download Minecraft + selected loader.</span
                >
              {:else}
                <span class="text-sm text-green-700"
                  >Ready to play — click <span class="font-semibold">Play</span> in the sidebar.</span
                >
              {/if}
              {#if installError}
                <span class="text-xs text-red-700 flex items-center gap-1">
                  {installError}
                  <button
                    class="text-neutral-500 hover:text-neutral-800"
                    onclick={() => (installError = null)}
                    aria-label="Dismiss"
                  >
                    ×
                  </button>
                </span>
              {/if}
              {#if exited && !running}
                <span class="text-xs text-neutral-600">Exited (code {exited.code})</span>
              {/if}
              {#if modsError}
                <span class="text-xs text-red-700 flex items-center gap-1">
                  {modsError}
                  <button
                    class="text-neutral-500 hover:text-neutral-800"
                    onclick={() => (modsError = null)}
                    aria-label="Dismiss"
                  >
                    ×
                  </button>
                </span>
              {/if}
            </div>
          {:else}
            <p class="text-sm text-neutral-500">
              No instance selected. Create one via the sidebar.
            </p>
          {/if}
        </div>
      {/snippet}
    </MainTabs>
  </div>

  <div class="col-start-1 col-end-3 row-start-2">
    <PhaseStatusRow />
  </div>

  <NetworkPopover bind:open={networkOpen} />
  <LogsPopover
    bind:open={logsOpen}
    initialPath={logsInitialPath}
    instanceId={activeInstance?.id ?? null}
  />

  <ManageInstancesModal
    bind:open={manageOpen}
    bind:instances
    bind:activeInstance
    {versions}
    onChanged={refreshInstances}
  />

  <SettingsModal />
  <TourOverlay />
</main>
<ToastHost />

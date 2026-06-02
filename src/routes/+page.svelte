<script lang="ts">
  import {
    commands,
    events,
    type Account,
    type CrashReport,
    type Error as IpcError,
    type InstanceWithStatus,
    type MissingModStatus,
    type ModpackProgress,
    type PlaytimeStats,
    type ProgressTick,
    type VersionEntry,
  } from '$lib/ipc/bindings';
  import { Channel } from '@tauri-apps/api/core';
  import { relativeTime } from '$lib/format/relative-time';
  import { formatDuration } from '$lib/format/duration';
  import PhaseStatusRow from '$lib/install/PhaseStatusRow.svelte';
  import LogsPopover from '$lib/logs/LogsPopover.svelte';
  import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';
  import SettingsModal from '$lib/settings/SettingsModal.svelte';
  import Sidebar from '$lib/layout/Sidebar.svelte';
  import MainTabs from '$lib/layout/MainTabs.svelte';
  import ExportPackDialog from '$lib/modpacks/ExportPackDialog.svelte';
  import ModpacksTab from '$lib/modpacks/ModpacksTab.svelte';
  import ModpacksModal from '$lib/modpacks/ModpacksModal.svelte';
  import ImportProgressView from '$lib/modpacks/ImportProgressView.svelte';
  import type { ModpackImportRequest } from '$lib/modpacks/import-request';
  import TourOverlay from '$lib/onboarding/TourOverlay.svelte';
  import ToastHost from '$lib/toasts/ToastHost.svelte';
  import MicrosoftSigningInModal from '$lib/accounts/MicrosoftSigningInModal.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { initOnboarding } from '$lib/onboarding/state.svelte';
  import { initTheme } from '$lib/theme/state.svelte';
  import { initLocale } from '$lib/i18n/state.svelte';
  import { t } from '$lib/i18n';
  import { get } from 'svelte/store';
  import { onMount, untrack } from 'svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { formatError } from '$lib/ipc/format-error';
  import { modBrowserNav, modpacksNav, mcVersions } from '$lib/settings/state.svelte';
  import {
    dismiss,
    pushActionToast,
    pushInfo,
    pushSuccess,
    pushWarning,
  } from '$lib/toasts/toasts.svelte';
  import { updateState, runUpdate, dismissUpdate } from '$lib/update/state.svelte';

  // How long the startup "new version available" toast stays before it
  // auto-hides. It only hides (reappears next launch); the durable path
  // is the Settings → Updates "Check for updates" button.
  const UPDATE_TOAST_TTL_MS = 5000;

  let accounts = $state<Account[]>([]);
  let activeAccount = $state<Account | null>(null);
  let offlineNameError = $state<string | null>(null);
  let listAccountsError = $state<string | null>(null);
  let removeError = $state<string | null>(null);

  // Vanilla MC version manifest, still fetched on mount and passed
  // through to ManageInstancesModal where it powers the MC version
  // picker. Home no longer renders a version dropdown — see Manage.
  let versions = $state<VersionEntry[]>([]);
  let versionsError = $state<string | null>(null);

  let instances = $state<InstanceWithStatus[]>([]);
  let activeInstance = $state<InstanceWithStatus | null>(null);
  let instancesError = $state<string | null>(null);

  let manageOpen = $state(false);
  let msSigningIn = $state(false);
  let exportDialogOpen = $state(false);

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

  // Per-instance playtime stats — refreshed on instance switch and
  // after every game exit (via the existing processExited handler).
  // last_session_unix_ms === null is the canonical "never played"
  // signal; the other fields can read null from the f64-via-specta
  // quirk and are coerced to 0 with ??.
  let playtime = $state<PlaytimeStats>({
    total_seconds: 0,
    session_count: 0,
    last_session_seconds: 0,
    last_session_unix_ms: null,
  });

  async function refreshPlaytime(id: string | null) {
    if (!id) {
      playtime = {
        total_seconds: 0,
        session_count: 0,
        last_session_seconds: 0,
        last_session_unix_ms: null,
      };
      return;
    }
    const r = await commands.getPlaytime(id);
    if (r.status === 'ok') playtime = r.data;
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

  // The modpacks browser floats above the always-present instance view as a
  // full-screen modal (ModpacksModal). It is not instance-scoped — installing a
  // pack creates a new instance — so a scrim-backed modal signals "separate
  // context, not the current instance".
  let modpacksModalOpen = $state(false);

  // Modpack import runs at the PAGE level (not inside ModpacksTab) so the modal
  // can be closed mid-import: the progress toast (ImportProgressView) lives here
  // and survives the modal unmounting. `modpackImporting` guards against a
  // second concurrent import; `importPhase` / `importBytes` drive the toast.
  let modpackImporting = $state(false);
  let importPhase = $state<ModpackProgress | null>(null);
  let importBytes = $state<ProgressTick | null>(null);

  // Run a modpack import handed up from ModpacksTab's picker. Owns the two
  // progress channels; on `done` it closes the modal (if still open) and lands
  // the user on the freshly created instance.
  async function runModpackImport(req: ModpackImportRequest) {
    if (modpackImporting) {
      const tr = get(t);
      pushWarning(tr('page.modpackImport.alreadyInProgress'), [
        tr('page.modpackImport.alreadyInProgressDetail'),
      ]);
      return;
    }
    modpackImporting = true;
    importPhase = null;
    importBytes = null;

    const phaseChannel = new Channel<ModpackProgress>();
    phaseChannel.onmessage = (m) => {
      importPhase = m;
      if (m.phase === 'done') {
        // Close the modal (if still open) and land on the new instance. The
        // `modpackImporting` guard is released below, after the command settles,
        // so a second import can't start in the gap between `done` and `Ok`.
        modpacksModalOpen = false;
        void onSelectInstance(m.instance_id);
      }
    };
    const tickChannel = new Channel<ProgressTick>();
    tickChannel.onmessage = (t) => {
      importBytes = t;
    };

    const r = await commands.modpackImport(
      req.path,
      req.selectedShas,
      true,
      req.projectId,
      req.source,
      req.versionId,
      phaseChannel,
      tickChannel,
    );
    // The phase channel emits `{ phase: 'done', instance_id }` and fires the
    // close + select above; the return value is only read for the error branch
    // (Rust guarantees `done` is emitted before Ok returns).
    const tr = get(t);
    if (r.status === 'ok') {
      pushSuccess(tr('page.modpackImport.imported', { name: r.data.name }));
    } else if (r.error.kind === 'modpack_partial_failure') {
      pushWarning(
        tr('page.modpackImport.partialFailure', { count: r.error.failed.length }),
        r.error.failed.map(([p]) => p.split('/').pop() ?? p),
      );
    } else {
      pushWarning(tr('page.modpackImport.failed'), [formatError(r.error)]);
    }
    // Reset in a single place once the run settles: holds the re-entrancy guard
    // for the whole import and clears the corner toast (on done or error).
    modpackImporting = false;
    importPhase = null;
    importBytes = null;
  }

  // The Overview missing-mods indicator and any other deep-link that
  // sets modpacksNav expects the Modpacks view to come up. ModpacksTab
  // itself reads the same rune to flip to the Imported sub-tab.
  $effect(() => {
    if (modpacksNav.value !== null) {
      modpacksModalOpen = true;
    }
  });

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
        void refreshPlaytime(newId);
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
        void refreshPlaytime(activeInstance?.id ?? null);
        if (event.payload.code !== 0 && activeInstance) {
          const result = await commands.latestCrash(activeInstance.id);
          if (result.status === 'ok' && result.data) {
            crashReport = result.data;
          }
        } else {
          crashReport = null;
        }
      })
      .then((u) => {
        exitUnlisten = u;
      });

    const settingsResult = await commands.appSettingsGet();
    if (settingsResult.status === 'ok') {
      initTheme(settingsResult.data.general.theme ?? 'system');
      initLocale(settingsResult.data.general.language ?? 'system');
    }

    // Fire-and-forget: this is a best-effort, error-swallowing check, so it
    // must NOT gate core init. Awaiting it here would stall accounts +
    // versions behind the network call (up to the client's 15s connect
    // timeout) on a GitHub outage. Let it resolve on its own; the toast
    // appears whenever it completes.
    if (settingsResult.status === 'ok' && settingsResult.data.general.check_updates_on_startup) {
      const dismissed = settingsResult.data.update_dismissed_version ?? null;
      void (async () => {
        const upd = await commands.updateCheck();
        if (upd.status === 'ok' && upd.data.available && upd.data.latest !== dismissed) {
          updateState.value = upd.data;
          const latest = upd.data.latest;
          const current = upd.data.current;
          const tr = get(t);
          const toastId = pushActionToast(
            'info',
            tr('page.update.available', { version: latest }),
            { label: tr('page.update.actionLabel'), run: () => void runUpdate() },
            [tr('page.update.currentVersion', { version: current })],
            () => void dismissUpdate(latest),
          );
          // Auto-hide the startup notification after a few seconds. This
          // only HIDES it (so it reappears next launch) — it does NOT mark
          // the version dismissed (that's the × button via dismissUpdate).
          // The durable path is the Settings → Updates "Check for updates"
          // button. No-op if the user already acted on the toast.
          setTimeout(() => dismiss(toastId), UPDATE_TOAST_TTL_MS);
        }
      })();
    }

    await refreshAccounts();

    const versionsResult = await commands.listVersions();
    if (versionsResult.status === 'ok') {
      versions = versionsResult.data;
      // Publish to the shared rune so the McVersionCombobox in the mod
      // and modpack browsers can read the list without prop drilling
      // through MainTabs / ModpacksTab.
      mcVersions.value = versionsResult.data;
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
      onOpenModpacks={() => (modpacksModalOpen = true)}
      {onSelectAccount}
      onRemoveAccount={onRemoveActive}
      onAddOffline={async (name) => {
        if (!name) {
          offlineNameError = get(t)('page.offlineName.cannotBeEmpty');
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
      {running}
      {installing}
      {onPlay}
      {onStop}
      {onInstall}
      bind:msSigningIn
      onMicrosoftSignedIn={async () => {
        await refreshAccounts();
        pushSuccess(get(t)('page.accounts.signedInMicrosoft'));
      }}
      onMicrosoftError={(err) => {
        const kind = (err as { kind?: string })?.kind;
        const msg = formatError(err as never);
        const tr = get(t);
        if (kind === 'auth_pending_approval') {
          pushInfo(tr('page.accounts.pendingApproval'), [msg]);
        } else {
          pushWarning(tr('page.accounts.signInFailed'), [msg]);
        }
      }}
    />
  </div>

  <div class="col-start-2 row-start-1 overflow-hidden flex flex-col">
    {#if crashReport}
      <div
        class="bg-danger-bg border-b border-danger text-danger px-4 py-2 flex items-center justify-between gap-3"
      >
        <span class="text-sm">
          {$t('page.crash.banner')}
          <span class="font-mono text-xs">{crashReport.path.split(/[\\/]/).pop()}</span>
        </span>
        <div class="flex items-center gap-2">
          <button class="btn-secondary btn-sm" onclick={openCrashInLogs}>
            {$t('page.crash.viewReport')}
          </button>
          <CloseButton
            onClick={() => (crashReport = null)}
            ariaLabel={$t('page.crash.dismissAriaLabel')}
          />
        </div>
      </div>
    {/if}

    <MainTabs
      instanceId={activeInstance?.id ?? null}
      mcVersion={activeInstance?.mc_version ?? null}
      loader={activeInstance?.loader ?? null}
      onListChanged={() => {
        void refreshInstances();
      }}
    >
      {#snippet overview()}
        <div class="p-6 flex flex-col gap-4">
          {#if offlineNameError}
            <p class="text-xs text-danger flex items-center gap-1">
              {offlineNameError}
              <CloseButton
                onClick={() => (offlineNameError = null)}
                ariaLabel={$t('page.overview.dismissError')}
              />
            </p>
          {/if}
          {#if listAccountsError}
            <p class="text-xs text-danger flex items-center gap-1">
              {listAccountsError}
              <CloseButton
                onClick={() => (listAccountsError = null)}
                ariaLabel={$t('page.overview.dismissError')}
              />
            </p>
          {/if}
          {#if removeError}
            <p class="text-xs text-danger flex items-center gap-1">
              {removeError}
              <CloseButton
                onClick={() => (removeError = null)}
                ariaLabel={$t('page.overview.dismissError')}
              />
            </p>
          {/if}
          {#if instancesError}
            <p class="text-xs text-danger">{instancesError}</p>
          {/if}
          {#if versionsError}
            <p class="text-xs text-danger">{versionsError}</p>
          {/if}
          {#if activeInstance}
            <div class="flex flex-col gap-1">
              <div class="text-xs uppercase tracking-wide text-muted">
                {$t('page.overview.sectionConfiguration')}
              </div>
              <div class="text-sm grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1">
                <span class="text-muted">{$t('page.overview.labelMinecraft')}</span>
                <span class="font-mono"
                  >{activeInstance.mc_version || $t('page.overview.notSet')}</span
                >
                <span class="text-muted">{$t('page.overview.labelLoader')}</span>
                <span class="font-mono">
                  {displayLoader(activeInstance.loader)}{#if activeInstance.loader_version}
                    · {activeInstance.loader_version}
                  {/if}
                </span>
                <span class="text-muted">{$t('page.overview.labelMemory')}</span>
                <span class="font-mono">{activeInstance.max_heap_mb} MB</span>
              </div>
              <p class="text-xs text-muted">
                {$t('page.overview.editVia')}
                <button type="button" class="btn-tertiary" onclick={() => (manageOpen = true)}
                  >{$t('page.overview.manageBtn')}</button
                >{$t('page.overview.editViaSuffix')}
              </p>
            </div>

            <div class="flex flex-col gap-1">
              <div class="text-xs uppercase tracking-wide text-muted">
                {$t('page.overview.sectionInstalledMods')}
              </div>
              {#if installedStats.total === 0}
                <p class="text-sm text-muted">
                  {$t('page.overview.noModsHint')}
                  <button
                    type="button"
                    class="btn-tertiary"
                    onclick={() => (modBrowserNav.value = { view: 'browse' })}
                  >
                    {$t('nav.modBrowser')}
                  </button>
                  {$t('page.overview.noModsHintSuffix')}
                </p>
              {:else}
                <div class="text-sm flex gap-3">
                  <span
                    >{$t('page.overview.statsTotal')}
                    <span class="font-medium text-secondary">{installedStats.total}</span></span
                  >
                  <span
                    >{$t('page.overview.statsEnabled')}
                    <span class="font-medium text-success">{installedStats.enabled}</span></span
                  >
                  <span
                    >{$t('page.overview.statsDisabled')}
                    <span class="font-medium text-secondary">{installedStats.disabled}</span></span
                  >
                </div>
                <p class="text-xs text-muted">
                  {$t('page.overview.manageInTab')}
                  <button
                    type="button"
                    class="btn-tertiary"
                    onclick={() => (modBrowserNav.value = { view: 'installed' })}
                  >
                    {$t('page.overview.installedTab')}
                  </button>
                  {$t('page.overview.manageInTabSuffix')}
                </p>
              {/if}
              {#if activeInstance && installedStats.enabled >= 1}
                <div class="mt-2">
                  <button
                    type="button"
                    class="btn-secondary btn-sm"
                    onclick={() => (exportDialogOpen = true)}
                  >
                    {$t('page.overview.exportModpack')}
                  </button>
                </div>
              {/if}
            </div>

            <div class="flex flex-col gap-1" data-testid="overview-playtime">
              <div class="text-xs uppercase tracking-wide text-muted">
                {$t('page.overview.sectionPlaytime')}
              </div>
              {#if playtime.last_session_unix_ms == null}
                <p class="text-sm text-muted">{$t('page.overview.notYetPlayed')}</p>
              {:else}
                <div class="text-sm">
                  {$t('page.overview.playtimeTotal')}
                  <span class="font-medium text-primary"
                    >{formatDuration(playtime.total_seconds)}</span
                  >
                  ·
                  <span
                    >{$t('page.overview.playtimeSession', { count: playtime.session_count })}</span
                  >
                </div>
                <p class="text-xs text-muted">
                  {$t('page.overview.playtimeLastSession', {
                    duration: formatDuration(playtime.last_session_seconds),
                    when: relativeTime(playtime.last_session_unix_ms),
                  })}
                </p>
              {/if}
            </div>

            {#if unresolvedMissing.length > 0}
              <button
                type="button"
                class="btn-warning-soft btn-sm w-full flex items-center gap-2 text-left"
                onclick={() => {
                  if (activeInstance) {
                    modpacksNav.value = { openDrawerForInstance: activeInstance.id };
                  }
                }}
                data-testid="overview-missing-mods"
              >
                <span aria-hidden="true">⚠</span>
                <span class="flex-1">
                  {$t('page.overview.missingModsBtn', { count: unresolvedMissing.length })}
                </span>
                <span class="text-xs text-warning-text underline"
                  >{$t('page.overview.missingModsView')}</span
                >
              </button>
            {/if}

            <div class="flex items-center gap-4 mt-2">
              {#if running}
                <span class="text-sm font-mono"
                  >{$t('page.overview.statusRunning', {
                    versionId: running.version_id,
                    pid: running.pid,
                  })}</span
                >
              {:else if activeInstance.mc_version === ''}
                <span class="text-sm text-muted"
                  >{$t('page.overview.statusPickVersion')}
                  <button type="button" class="btn-tertiary" onclick={() => (manageOpen = true)}
                    >{$t('page.overview.manageBtn')}</button
                  >
                  {$t('page.overview.statusPickVersionSuffix')}</span
                >
              {:else if installing}
                <span class="text-sm text-accent">{$t('page.overview.statusWorking')}</span>
              {:else if !activeInstance.ready}
                <span class="text-sm text-muted"
                  >{$t('page.overview.statusInstallHint')}
                  <span class="font-semibold text-secondary"
                    >{$t('page.overview.statusInstallLabel')}</span
                  >
                  {$t('page.overview.statusInstallHintSuffix')}</span
                >
              {:else}
                <span class="text-sm text-success"
                  >{$t('page.overview.statusReady')}
                  <span class="font-semibold">{$t('page.overview.statusReadyPlayLabel')}</span>
                  {$t('page.overview.statusReadySuffix')}</span
                >
              {/if}
              {#if installError}
                <span class="text-xs text-danger flex items-center gap-1">
                  {installError}
                  <CloseButton
                    onClick={() => (installError = null)}
                    ariaLabel={$t('page.overview.dismissError')}
                  />
                </span>
              {/if}
              {#if exited && !running}
                <span class="text-xs text-secondary"
                  >{$t('page.overview.statusExited', { code: exited.code })}</span
                >
              {/if}
              {#if modsError}
                <span class="text-xs text-danger flex items-center gap-1">
                  {modsError}
                  <CloseButton
                    onClick={() => (modsError = null)}
                    ariaLabel={$t('page.overview.dismissError')}
                  />
                </span>
              {/if}
            </div>
          {:else}
            <p class="text-sm text-muted">{$t('page.overview.noInstanceSelected')}</p>
          {/if}
        </div>
      {/snippet}
    </MainTabs>
  </div>

  <div class="col-start-1 col-end-3 row-start-2">
    <PhaseStatusRow />
  </div>

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
  <ModpacksModal open={modpacksModalOpen} onClose={() => (modpacksModalOpen = false)}>
    <ModpacksTab
      {instances}
      onImport={runModpackImport}
      onInstanceCreated={(id) => {
        // Opening an imported pack's instance from the Imported tab: close the
        // modal and land on it.
        modpacksModalOpen = false;
        void onSelectInstance(id);
      }}
      onListChanged={() => {
        void refreshInstances();
      }}
    />
  </ModpacksModal>
  <!-- Page-level import progress toast — lives outside the modal so it survives
       the modal being closed mid-import. Renders nothing when no import runs. -->
  <ImportProgressView phase={importPhase} modBytes={importBytes} />
  <TourOverlay />
  {#if exportDialogOpen && activeInstance}
    <ExportPackDialog
      instanceId={activeInstance.id}
      instanceName={activeInstance.name}
      onClose={() => (exportDialogOpen = false)}
    />
  {/if}
</main>
<ToastHost />
<MicrosoftSigningInModal
  open={msSigningIn}
  onCancel={() => {
    msSigningIn = false;
  }}
/>

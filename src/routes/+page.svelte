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
  import PhaseStatusRow from '$lib/install/PhaseStatusRow.svelte';
  import LogsPopover from '$lib/logs/LogsPopover.svelte';
  import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';
  import SettingsModal from '$lib/settings/SettingsModal.svelte';
  import Sidebar from '$lib/layout/Sidebar.svelte';
  import MainTabs from '$lib/layout/MainTabs.svelte';
  import OverviewTab from '$lib/overview/OverviewTab.svelte';
  import ExportPackDialog from '$lib/modpacks/ExportPackDialog.svelte';
  import ModpacksTab from '$lib/modpacks/ModpacksTab.svelte';
  import ModpacksModal from '$lib/modpacks/ModpacksModal.svelte';
  import ImportProgressView from '$lib/modpacks/ImportProgressView.svelte';
  import IntegrityProgressView from '$lib/instances/IntegrityProgressView.svelte';
  import { integrityCompletionTick } from '$lib/instances/integrity-ops.svelte';
  import type { ModpackImportRequest } from '$lib/modpacks/import-request';
  import TourOverlay from '$lib/onboarding/TourOverlay.svelte';
  import ToastHost from '$lib/toasts/ToastHost.svelte';
  import MicrosoftSigningInModal from '$lib/accounts/MicrosoftSigningInModal.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { initOnboarding, showAccountHint } from '$lib/onboarding/state.svelte';
  import { explanationState } from '$lib/onboarding/explanation-level.svelte';
  import { initTheme } from '$lib/theme/state.svelte';
  import { initLocale } from '$lib/i18n/state.svelte';
  import { t } from '$lib/i18n';
  import { get } from 'svelte/store';
  import { onMount, untrack } from 'svelte';
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

  // Offline incompatible-mod count for the Overview indicator (network-free).
  // Counts ONLY manual jars whose loader family mismatches (`!live_checkable`) —
  // those are the definitive offline verdicts. Platform suspects need the live
  // auto-confirm the Installed tab performs (the Overview makes no network call),
  // so counting their raw offline suspicion here would re-introduce false
  // positives. Empty for vanilla / version-less instances.
  let incompatibleCount = $state(0);
  async function refreshIncompatible(id: string | null) {
    const inst = id ? instances.find((i) => i.id === id) : null;
    if (!inst || !inst.mc_version || inst.loader === 'vanilla') {
      incompatibleCount = 0;
      return;
    }
    const r = await commands.scanInstanceModCompat(inst.id, inst.mc_version, inst.loader);
    incompatibleCount =
      r.status === 'ok' ? r.data.filter((x) => x.loader_mismatch && !x.live_checkable).length : 0;
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
        void refreshIncompatible(newId);
        void refreshPackStatus(newId);
        void refreshPlaytime(newId);
      }
    });
  });

  // When a background integrity verify/repair finishes, the store bumps
  // `completionTick`. Refresh the instance list so the persisted status
  // (badge + Overview + the Manage section's reactive `status`) updates,
  // independent of any modal/section lifecycle. Skip the very first run
  // (mount) so this doesn't double-fetch alongside onMount's refresh.
  let integritySettled = false;
  $effect(() => {
    void integrityCompletionTick();
    if (integritySettled) {
      void refreshInstances();
    } else {
      integritySettled = true;
    }
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
      void refreshIncompatible(activeInstance?.id ?? null);
      void refreshPackStatus(activeInstance?.id ?? null);
    });
    events.modUninstalled.listen(() => {
      void refreshInstalledStats(activeInstance?.id ?? null);
      void refreshIncompatible(activeInstance?.id ?? null);
      void refreshPackStatus(activeInstance?.id ?? null);
    });
    events.modToggle.listen(() => {
      void refreshInstalledStats(activeInstance?.id ?? null);
      void refreshIncompatible(activeInstance?.id ?? null);
    });

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
      explanationState.level = settingsResult.data.general.explanation_level ?? 'basic';
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
    // No account = the game can't launch. Instead of the backend's terse
    // AccountNotSet error in a tiny banner, spotlight the ACCOUNT section and
    // explain the two paths (Microsoft / offline). Fires every time.
    if (!activeAccount) {
      showAccountHint();
      return;
    }
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
      instanceName={activeInstance?.name ?? null}
      mcVersion={activeInstance?.mc_version ?? null}
      loader={activeInstance?.loader ?? null}
      onListChanged={() => {
        void refreshInstances();
      }}
    >
      {#snippet overview()}
        <OverviewTab
          {activeInstance}
          {installedStats}
          {playtime}
          {incompatibleCount}
          missingModsCount={unresolvedMissing.length}
          running={running !== null}
          {installing}
          {exited}
          {installError}
          {modsError}
          errors={{
            offlineName: offlineNameError,
            listAccounts: listAccountsError,
            remove: removeError,
            instances: instancesError,
            versions: versionsError,
          }}
          onManage={() => (manageOpen = true)}
          onExport={() => (exportDialogOpen = true)}
          onOpenPackDrawer={() => {
            if (activeInstance) modpacksNav.value = { openDrawerForInstance: activeInstance.id };
          }}
          onNavInstalled={() => (modBrowserNav.value = { view: 'installed' })}
          onNavBrowse={() => (modBrowserNav.value = { view: 'browse' })}
          onDismissError={(key) => {
            if (key === 'offlineName') offlineNameError = null;
            else if (key === 'listAccounts') listAccountsError = null;
            else if (key === 'remove') removeError = null;
            else if (key === 'instances') instancesError = null;
            else if (key === 'versions') versionsError = null;
          }}
          onDismissInstallError={() => (installError = null)}
          onDismissModsError={() => (modsError = null)}
        />
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
    instanceName={activeInstance?.name ?? null}
  />

  <ManageInstancesModal
    bind:open={manageOpen}
    bind:instances
    bind:activeInstance
    {versions}
    onChanged={refreshInstances}
    isRunning={running !== null}
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
  <!-- Page-level integrity verify/repair progress — like the import view, lives
       outside the Manage modal so a background op stays visible after close. -->
  <IntegrityProgressView />
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

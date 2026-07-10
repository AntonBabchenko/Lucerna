<script lang="ts">
  import {
    commands,
    events,
    type Account,
    type CrashReport,
    type Error as IpcError,
    type InstanceWithStatus,
  } from '$lib/ipc/bindings';
  import PhaseStatusRow from '$lib/install/PhaseStatusRow.svelte';
  import LogsPopover from '$lib/logs/LogsPopover.svelte';
  import { drainDeferredRepairs } from '$lib/logs/deferred-repairs.svelte';
  import { refreshDiagnosis } from '$lib/logs/log-diagnosis.svelte';
  import { repairCompletionTick } from '$lib/logs/repair-ops.svelte';
  import InstanceIconDialog from '$lib/instances/InstanceIconDialog.svelte';
  import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';
  import SettingsModal from '$lib/settings/SettingsModal.svelte';
  import Sidebar from '$lib/layout/Sidebar.svelte';
  import {
    compactState,
    initCompact,
    observeCompactContent,
    setCompact,
    toggleCompact,
  } from '$lib/layout/compact.svelte';
  import { initSidebarButtons } from '$lib/layout/sidebar-buttons.svelte';
  import MainTabs from '$lib/layout/MainTabs.svelte';
  import OverviewTab from '$lib/overview/OverviewTab.svelte';
  import ExportPackDialog from '$lib/modpacks/ExportPackDialog.svelte';
  import LauncherImportDialog from '$lib/instances/import/LauncherImportDialog.svelte';
  import ModpacksTab from '$lib/modpacks/ModpacksTab.svelte';
  import ModpacksModal from '$lib/modpacks/ModpacksModal.svelte';
  import ServersPanel from '$lib/servers/ServersPanel.svelte';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { serversUi } from '$lib/servers/servers-ui.svelte';
  import ScreenshotsGallery from '$lib/screenshots/ScreenshotsGallery.svelte';
  import OperationsView from '$lib/ops/OperationsView.svelte';
  import {
    enqueueImport,
    opCompletionTick,
    opImportCompletionTick,
  } from '$lib/ops/op-queue.svelte';
  import { createInstanceStats } from '$lib/instances/instance-stats.svelte';
  import TourOverlay from '$lib/onboarding/TourOverlay.svelte';
  import ToastHost from '$lib/toasts/ToastHost.svelte';
  import MicrosoftSigningInModal from '$lib/accounts/MicrosoftSigningInModal.svelte';
  import RemoveAccountDialog from '$lib/accounts/RemoveAccountDialog.svelte';
  import AddOfflineAccountDialog from '$lib/accounts/AddOfflineAccountDialog.svelte';
  import SkinCapeModal from '$lib/accounts/SkinCapeModal.svelte';
  import QuickJoinDialog from '$lib/worlds/QuickJoinDialog.svelte';
  import PreflightGateDialog from '$lib/mods/PreflightGateDialog.svelte';
  import { decideLaunch, remediateAll } from '$lib/mods/preflight.svelte';
  import type { PreflightReport } from '$lib/ipc/bindings';
  import { classifySignInError } from '$lib/accounts/sign-in-error';
  import { quickPlayDisabledKey } from '$lib/worlds/quick-play-gating';
  import { createQuickWorlds } from '$lib/worlds/quick-worlds.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { initOnboarding, showAccountHint } from '$lib/onboarding/state.svelte';
  import { explanationState } from '$lib/onboarding/explanation-level.svelte';
  import { initTheme } from '$lib/theme/state.svelte';
  import { initLocale } from '$lib/i18n/state.svelte';
  import { t } from '$lib/i18n';
  import { get } from 'svelte/store';
  import { onDestroy, onMount, untrack } from 'svelte';
  import { formatError } from '$lib/ipc/format-error';
  import { modBrowserNav, modpacksNav, settingsOpen } from '$lib/settings/state.svelte';
  import { createMcVersions } from '$lib/versions/mc-versions.svelte';
  import {
    dismiss,
    pushActionToast,
    pushInfo,
    pushSuccess,
    pushWarning,
  } from '$lib/toasts/toasts.svelte';
  import { updateState, runUpdate, dismissUpdate } from '$lib/update/state.svelte';
  import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import { dataRootPlayDisabledKey } from '$lib/settings/data-root-gating';
  import DataRootFallbackBanner from '$lib/settings/DataRootFallbackBanner.svelte';

  // How long the startup "new version available" toast stays before it
  // auto-hides. It only hides (reappears next launch); the durable path
  // is the Settings → Updates "Check for updates" button.
  const UPDATE_TOAST_TTL_MS = 5000;

  let accounts = $state<Account[]>([]);
  let activeAccount = $state<Account | null>(null);
  let offlineNameError = $state<string | null>(null);
  // Open-state for the add-offline-account dialog. The dialog owns the name
  // entry; the submit handler below keeps it open and shows offlineNameError on
  // a backend failure, and closes it on success.
  let addOfflineOpen = $state(false);
  let listAccountsError = $state<string | null>(null);
  let removeError = $state<string | null>(null);
  // Id of the account pending removal — gates the actual delete behind the
  // confirm dialog (see RemoveAccountDialog). Set from the per-row trash in the
  // account dropdown; any account can be removed, not just the active one.
  let removeConfirmId = $state<string | null>(null);
  // Account whose skin & cape editor is open (Microsoft accounts only). Set from
  // the sidebar cosmetics button; cleared when the SkinCapeModal closes.
  let cosmeticsAccount = $state<Account | null>(null);
  const removeConfirmAccount = $derived(
    removeConfirmId ? (accounts.find((a) => a.id === removeConfirmId) ?? null) : null,
  );

  // Self-healing owner of the MC version manifest (fetch + online/backoff
  // recovery). Powers the Manage modal's version picker and publishes to the
  // shared `mcVersions` rune for the browsers' version combobox.
  const mcv = createMcVersions();

  let instances = $state<InstanceWithStatus[]>([]);
  let activeInstance = $state<InstanceWithStatus | null>(null);
  let instancesError = $state<string | null>(null);

  // Mirrors `general.check_updates_on_startup`: when off, no background modpack
  // update sweeps run (offline-first / privacy). Set once settings load.
  let modpackSweepEnabled = $state(false);

  // Best-effort background sweep of imported packs for available updates.
  // Gated by the same setting as the app self-update check. Errors are
  // swallowed inside the store; the store's TTL dedups repeated calls.
  function sweepModpackUpdates(force = false) {
    if (!modpackSweepEnabled) return;
    const packIds = instances.filter((i) => i.mrpack_name != null).map((i) => i.id);
    void modpackUpdates.sweep(packIds, { force });
  }

  let manageOpen = $state(false);
  // Seeds ManageInstancesModal's detail selection. Set to a specific instance id
  // when opened via a per-row "manage this profile" action (the active-instance
  // switch is async, so the modal can't rely on `activeInstance` at open time);
  // null when opened via the generic Manage button (defaults to the active one).
  let manageInitialId = $state<string | null>(null);
  let msSigningIn = $state(false);
  let exportDialogOpen = $state(false);

  // Per-instance Overview stats (installed-mod counts, incompatible count,
  // playtime, pack-missing mods) live in a dedicated rune composable. The page
  // drives its refreshers from the activeInstance effect, the mod
  // install/uninstall/toggle listeners, and the processExited handler.
  const stats = createInstanceStats();

  let installing = $state(false);
  let installError = $state<string | null>(null);
  let running = $state<{ pid: number; version_id: string } | null>(null);
  let exited = $state<{ code: number; user_requested: boolean; log_path: string } | null>(null);
  // Tauri event unlisteners, captured so the listeners are torn down on unmount
  // rather than leaking across the page's lifetime. (This is a long-lived
  // single-page shell, but the listeners still need explicit cleanup — an
  // unmounted-then-remounted page would otherwise double-subscribe.)
  let spawnUnlisten: (() => void) | null = null;
  let exitUnlisten: (() => void) | null = null;
  let modInstalledUnlisten: (() => void) | null = null;
  let modUninstalledUnlisten: (() => void) | null = null;
  let modToggleUnlisten: (() => void) | null = null;

  let quickPlaySupported = $state(false);
  let quickJoinOpen = $state(false);
  let quickJoinBusy = $state(false);
  let savedServers = $state<import('$lib/ipc/bindings').SavedServer[]>([]);
  let savedServersLoading = $state(false);

  async function loadSavedServers() {
    savedServersLoading = true;
    try {
      if (!activeInstance) {
        savedServers = [];
        return;
      }
      const r = await commands.listSavedServers(activeInstance.id);
      savedServers = r.status === 'ok' ? r.data : [];
    } finally {
      savedServersLoading = false;
    }
  }

  async function openServersDialog() {
    quickJoinOpen = true;
    await loadSavedServers();
  }

  // Pre-flight gate: populated when hasBlocking violations are found before launch.
  let gateReport = $state<PreflightReport | null>(null);
  let gateBusy = $state(false);

  let logsOpen = $state(false);
  let screenshotsGalleryOpen = $state(false);
  let logsInitialPath = $state<string | null>(null);
  let crashReport = $state<CrashReport | null>(null);
  let modsError = $state<string | null>(null);

  // The modpacks browser floats above the always-present instance view as a
  // full-screen modal (ModpacksModal). It is not instance-scoped — installing a
  // pack creates a new instance — so a scrim-backed modal signals "separate
  // context, not the current instance".
  let modpacksModalOpen = $state(false);
  let launcherImportOpen = $state(false);

  // True once the first server-list load has settled — gates reconcile() so a
  // pre-load empty list can't wipe the persisted selection.
  let serversReady = $state(false);

  // The Overview missing-mods indicator and any other deep-link that
  // sets modpacksNav expects the Modpacks view to come up. ModpacksTab
  // itself reads the same rune to flip to the Imported sub-tab.
  $effect(() => {
    if (modpacksNav.value !== null) {
      modpacksModalOpen = true;
    }
  });

  // Keep the persisted server selection honest against the live list (deleted
  // server, fresh profile, list refreshes). reconcile() writes only when the
  // selection is actually invalid, so this cannot loop.
  $effect(() => {
    if (!serversReady) return;
    serversUi.reconcile(serverState.list.map((s) => s.id));
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
        void stats.refreshInstalledStats(newId);
        void stats.refreshIncompatible(newId, instances);
        void stats.refreshPackStatus(newId);
        void stats.refreshPlaytime(newId);
      }
    });
  });

  // Refresh quickPlaySupported whenever the active instance changes or becomes
  // ready. The effect tracks activeInstance reactively; the async callback
  // guards against stale results by checking the id hasn't changed.
  $effect(() => {
    const id = activeInstance?.id;
    const ready = activeInstance?.ready ?? false;
    if (!id || !ready) {
      quickPlaySupported = false;
      return;
    }
    void commands.instanceQuickPlaySupport(id).then((r) => {
      if (activeInstance?.id !== id) return; // ignore stale async result
      quickPlaySupported = r.status === 'ok' ? r.data : false;
    });
  });

  // When a background integrity verify/repair finishes, the store bumps
  // `completionTick`. Refresh the instance list so the persisted status
  // (badge + Overview + the Manage section's reactive `status`) updates,
  // independent of any modal/section lifecycle. Skip the very first run
  // (mount) so this doesn't double-fetch alongside onMount's refresh.
  let integritySettled = false;
  $effect(() => {
    void opCompletionTick();
    if (integritySettled) {
      void refreshInstances();
    } else {
      integritySettled = true;
    }
  });

  // A queued modpack import just landed a new instance. Refresh so the pack is
  // in `instances`, then FORCE a modpack-update sweep — the store dedups by a
  // single global TTL, so a freshly-imported pack would otherwise be skipped if
  // a sweep already ran this session. Skip the mount run (tick starts at 0).
  let importSettled = false;
  $effect(() => {
    void opImportCompletionTick();
    if (importSettled) {
      void refreshInstances().then(() => sweepModpackUpdates(true));
    } else {
      importSettled = true;
    }
  });

  // Keep the latest-log diagnosis indicator fresh whenever the active instance
  // changes. The effect re-runs reactively because it reads activeInstance?.id.
  $effect(() => {
    void refreshDiagnosis(activeInstance?.id ?? null);
  });
  // Re-check when a repair completes (the log it wrote may now be analysed).
  $effect(() => {
    void repairCompletionTick(); // track the tick reactively
    const id = activeInstance?.id;
    if (id) void refreshDiagnosis(id);
  });

  // Any wide overlay opened while compact would be cramped in the strip-width
  // window, so auto-expand first. Covers the Settings button too (it sets
  // settingsOpen.value directly inside Sidebar). This is a real expand — it
  // persists compact_mode=false; the user re-collapses with the toggle.
  $effect(() => {
    const anyWideOverlay =
      manageOpen ||
      modpacksModalOpen ||
      launcherImportOpen ||
      logsOpen ||
      screenshotsGalleryOpen ||
      settingsOpen.value !== null ||
      exportDialogOpen ||
      msSigningIn;
    // Read compact state non-reactively (matches the activeInstance effect's
    // untrack idiom above): we react to overlays opening, not to our own
    // setCompact(false) flipping the rune.
    if (anyWideOverlay && untrack(() => compactState.value)) {
      void setCompact(false);
    }
  });

  const quickPlayDisabledReason = $derived.by(() => {
    const key = quickPlayDisabledKey({
      ready: activeInstance?.ready ?? false,
      running: running !== null,
      supported: quickPlaySupported,
    });
    return key === null ? null : get(t)(key);
  });

  // Blocks Play/Install (and Quick Play/Quick Join, via the same reason
  // string) while the configured data root is unavailable — see §7 of the
  // data-root design doc and data-root-gating.ts. `null` = not blocked.
  const dataRootPlayBlockedReason = $derived.by(() => {
    const key = dataRootPlayDisabledKey(dataLocation.fellBack);
    return key === null ? null : get(t)(key);
  });

  const quickWorlds = createQuickWorlds();
  onDestroy(() => quickWorlds.dispose());

  // The dropdown is usable only when a world can actually be quick-launched:
  // Quick Play supported by this MC version, the instance installed, and the
  // game not already running.
  const quickPlayMenuEnabled = $derived(
    quickPlaySupported && (activeInstance?.ready ?? false) && running === null,
  );

  // Load the cheap world list when the active instance is eligible; drop it
  // otherwise. Loads regardless of `running` (worlds only change on exit,
  // which the composable already re-fetches on); `menuEnabled` gates display.
  $effect(() => {
    const id = activeInstance?.id ?? null;
    if (id && quickPlaySupported && (activeInstance?.ready ?? false)) {
      quickWorlds.load(id);
    } else {
      quickWorlds.clear();
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

  // Keep the compact strip's height synced to its live content (status row
  // appearing/disappearing, etc.). Separate from the async onMount below so
  // Svelte actually uses the returned disposer for cleanup.
  onMount(() => observeCompactContent());

  onMount(async () => {
    void dataLocation.init();
    void refreshInstances();

    // Servers now live in a persistent panel (not a modal), so the store boots
    // with the page: subscribe to events once, then load the list and only
    // then let reconcile() touch the persisted selection.
    serverState.init();
    void serverState.refresh().then(() => {
      serversReady = true;
    });

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
    events.modInstalled
      .listen(() => {
        void stats.refreshInstalledStats(activeInstance?.id ?? null);
        void stats.refreshIncompatible(activeInstance?.id ?? null, instances);
        void stats.refreshPackStatus(activeInstance?.id ?? null);
      })
      .then((u) => {
        modInstalledUnlisten = u;
      });
    events.modUninstalled
      .listen(() => {
        void stats.refreshInstalledStats(activeInstance?.id ?? null);
        void stats.refreshIncompatible(activeInstance?.id ?? null, instances);
        void stats.refreshPackStatus(activeInstance?.id ?? null);
      })
      .then((u) => {
        modUninstalledUnlisten = u;
      });
    events.modToggle
      .listen(() => {
        void stats.refreshInstalledStats(activeInstance?.id ?? null);
        void stats.refreshIncompatible(activeInstance?.id ?? null, instances);
      })
      .then((u) => {
        modToggleUnlisten = u;
      });

    events.processExited
      .listen(async (event) => {
        running = null;
        // Apply any repairs the user queued while the game was running (their
        // files were locked); now the instance is free.
        void drainDeferredRepairs();
        // A new log was written — refresh the diagnosis indicator.
        if (activeInstance) void refreshDiagnosis(activeInstance.id);
        exited = {
          code: event.payload.code,
          user_requested: event.payload.user_requested,
          log_path: event.payload.log_path,
        };
        void refreshInstances();
        void stats.refreshPlaytime(activeInstance?.id ?? null);
        // A user-requested Stop force-kills the process (non-zero exit code),
        // but it is not a crash — don't surface a crash diagnosis for it.
        if (event.payload.code !== 0 && !event.payload.user_requested && activeInstance) {
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
      void initCompact(settingsResult.data.general.compact_mode ?? false);
      initSidebarButtons(settingsResult.data.general.hidden_sidebar_buttons ?? []);
      modpackSweepEnabled = settingsResult.data.general.check_updates_on_startup ?? true;
      sweepModpackUpdates();
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

    // Fire-and-forget: the composable owns the fetch, publishes the list to the
    // shared `mcVersions` rune, and self-heals a transient failure (online event
    // + bounded backoff) instead of leaving a stale error banner.
    void mcv.load();

    void initOnboarding();
  });

  onDestroy(() => mcv.dispose());

  // Tear down every Tauri event listener registered in onMount. Without this the
  // stored unlisteners were never called and the mod-event listeners were never
  // even captured, so they leaked on unmount.
  onDestroy(() => {
    spawnUnlisten?.();
    exitUnlisten?.();
    modInstalledUnlisten?.();
    modUninstalledUnlisten?.();
    modToggleUnlisten?.();
  });

  async function onSelectAccount(id: string) {
    const result = await commands.setActiveAccount(id);
    if (result.status === 'ok') {
      await refreshAccounts();
    }
  }

  function requestRemoveAccount(id: string) {
    removeError = null;
    removeConfirmId = id;
  }

  async function confirmRemoveAccount() {
    const id = removeConfirmId;
    removeConfirmId = null;
    if (!id) return;
    removeError = null;
    const result = await commands.removeAccount(id);
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
    sweepModpackUpdates();
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
    if (dataLocation.fellBack) return;
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

  // Shared launch path: used by the normal (no violations) flow AND by
  // the gate dialog's "Launch anyway" / post-update path.
  async function doLaunch() {
    if (!activeInstance) return;
    installError = null;
    const result = await commands.launchInstance(activeInstance.id, null);
    if (result.status === 'error') {
      installError = formatError(result.error);
    }
    // processSpawned event sets `running` once MC starts; processExited
    // clears it. No need to refresh state here.
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
    if (dataLocation.fellBack) return;

    // Dependency pre-flight: check for blocking violations before launch.
    // Fail-open: if the command errors, proceed normally (decideLaunch returns 'launch').
    const pr = await commands.instanceDependencyPreflight(activeInstance.id);
    if (decideLaunch(pr) === 'gate') {
      // pr.status === 'ok' is guaranteed here (decideLaunch only returns 'gate' on ok+violations)
      gateReport = (pr as { status: 'ok'; data: PreflightReport }).data;
      return;
    }

    await doLaunch();
  }

  // Gate dialog handlers
  async function onGateLaunchAnyway() {
    gateReport = null;
    await doLaunch();
  }

  async function onGateUpdateLaunch() {
    if (!activeInstance || !gateReport) return;
    gateBusy = true;
    const loader = activeInstance.loader;
    const mc = activeInstance.mc_version;
    const updated = await remediateAll(activeInstance.id, gateReport, mc, loader);
    gateBusy = false;
    if (updated === 0) {
      // Nothing was fixed (offline, no compatible version, etc.) — keep the gate
      // dialog open so the user can choose "Launch anyway" or "Cancel" explicitly.
      pushWarning(get(t)('mods.preflight.updateFailed'));
      return;
    }
    gateReport = null;
    await doLaunch();
  }

  function onGateCancel() {
    gateReport = null;
  }

  async function onQuickPlayWorld(folderName: string) {
    if (!activeInstance) return;
    if (!activeAccount) {
      showAccountHint();
      return;
    }
    if (quickPlayDisabledReason !== null) return;
    if (dataLocation.fellBack) return;
    installError = null;
    const result = await commands.launchInstance(activeInstance.id, {
      kind: 'singleplayer',
      world: folderName,
    });
    if (result.status === 'error') {
      installError = formatError(result.error);
    }
  }

  async function connectToAddress(address: string) {
    if (!activeInstance) return;
    if (!activeAccount) {
      showAccountHint();
      return;
    }
    if (quickPlayDisabledReason !== null) return;
    if (dataLocation.fellBack) return;
    quickJoinBusy = true;
    installError = null;
    const result = await commands.launchInstance(activeInstance.id, {
      kind: 'multiplayer',
      address,
    });
    quickJoinBusy = false;
    if (result.status === 'error') {
      installError = formatError(result.error);
    } else {
      quickJoinOpen = false;
    }
  }

  async function onServerSave(name: string, address: string): Promise<boolean> {
    if (!activeInstance) return false;
    quickJoinBusy = true;
    installError = null;
    const r = await commands.addSavedServer(activeInstance.id, name, address);
    quickJoinBusy = false;
    if (r.status === 'error') {
      installError = formatError(r.error);
      return false;
    }
    await loadSavedServers();
    return true;
  }

  async function onServerSaveAndConnect(name: string, address: string): Promise<boolean> {
    if (!activeInstance) return false;
    quickJoinBusy = true;
    installError = null;
    const r = await commands.addSavedServer(activeInstance.id, name, address);
    if (r.status === 'error') {
      quickJoinBusy = false;
      installError = formatError(r.error);
      return false;
    }
    await loadSavedServers();
    quickJoinBusy = false;
    await connectToAddress(address);
    return true;
  }

  async function onServerDelete(index: number, address: string) {
    if (!activeInstance) return;
    quickJoinBusy = true;
    installError = null;
    const r = await commands.removeSavedServer(activeInstance.id, index, address);
    quickJoinBusy = false;
    if (r.status === 'error') {
      installError = formatError(r.error);
    }
    await loadSavedServers();
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
  style="grid-template-columns: {compactState.value
    ? '1fr'
    : '240px 1fr'}; grid-template-rows: 1fr auto;"
>
  <!--
    Expanded: the sidebar spans BOTH grid rows (`grid-row: 1 / -1`) so the
    install/mod status row (row 2, content column only) never steals its height.
    The sidebar is sized exactly to its content via the min-height floor, so
    losing any row height would force it into a scrollbar. Compact: single
    column, the status row stacks below the sidebar, so it stays in row 1.
  -->
  <div class="col-start-1 overflow-hidden" style="grid-row: {compactState.value ? '1' : '1 / -1'};">
    <Sidebar
      {accounts}
      {activeAccount}
      {instances}
      {activeInstance}
      compact={compactState.value}
      onToggleCompact={() => void toggleCompact()}
      onOpenModpacks={() => (modpacksModalOpen = true)}
      onOpenGallery={() => (screenshotsGalleryOpen = true)}
      onOpenLauncherImport={() => (launcherImportOpen = true)}
      onOpenQuickJoin={() => void openServersDialog()}
      {onSelectAccount}
      onRemoveAccount={requestRemoveAccount}
      onOpenCosmetics={(account) => (cosmeticsAccount = account)}
      onAddOffline={() => {
        offlineNameError = null;
        addOfflineOpen = true;
      }}
      {onSelectInstance}
      onOpenManage={() => {
        manageInitialId = null;
        manageOpen = true;
      }}
      onManageInstance={(id) => {
        manageInitialId = id;
        manageOpen = true;
      }}
      {onOpenMods}
      onOpenLogs={() => {
        // Plain "Logs" open is not a deep-link — clear any stale crash path so
        // the viewer selects the newest log instead of re-opening an old crash.
        logsInitialPath = null;
        logsOpen = !logsOpen;
      }}
      {running}
      {installing}
      {onPlay}
      {onStop}
      {onInstall}
      worlds={quickWorlds.worlds}
      {onQuickPlayWorld}
      {quickPlayMenuEnabled}
      playBlockedReason={dataRootPlayBlockedReason}
      createBlockedReason={dataLocation.fellBack
        ? get(t)('page.dataRootFallback.createDisabledReason')
        : null}
      launcherImportBlockedReason={dataLocation.fellBack
        ? get(t)('page.dataRootFallback.createDisabledReason')
        : null}
      dataRootFallbackReason={compactState.value && dataLocation.fellBack && dataLocation.status
        ? get(t)('page.dataRootFallback.banner', { path: dataLocation.status.configured ?? '' })
        : null}
      bind:msSigningIn
      onMicrosoftSignedIn={async () => {
        await refreshAccounts();
        pushSuccess(get(t)('page.accounts.signedInMicrosoft'));
      }}
      onMicrosoftError={(err) => {
        const msg = formatError(err as never);
        const tr = get(t);
        const toast = classifySignInError(err);
        if (toast.buyLink) {
          const url = toast.buyLink;
          pushActionToast(
            toast.kind,
            tr(toast.titleKey),
            {
              label: tr('page.accounts.buyMinecraft'),
              run: () => void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url)),
            },
            [msg],
          );
        } else if (toast.kind === 'info') {
          pushInfo(tr(toast.titleKey), [msg]);
        } else {
          pushWarning(tr(toast.titleKey), [msg]);
        }
      }}
    />
  </div>

  <!-- Compact mode unmounts the entire right column so the window can shrink to
       the sidebar strip. Note: this resets MainTabs (active tab / scroll) on
       re-expand — acceptable because compact is a launch-pad mode, not a rapid
       toggle-while-browsing-tabs affordance. Both mode panels (client MainTabs
       and ServersPanel) share the same grid cell and stay mounted; the inactive
       one gets `hidden` (display:none) so tab state, console scroll and wizard
       progress survive mode switches — compact still unmounts the whole column. -->
  {#if !compactState.value}
    <div
      class="col-start-2 row-start-1 overflow-hidden flex flex-col"
      hidden={serversUi.mode !== 'client'}
    >
      {#if dataLocation.fellBack && dataLocation.status}
        <DataRootFallbackBanner configuredPath={dataLocation.status.configured ?? ''} />
      {/if}
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
        {onQuickPlayWorld}
        {quickPlayDisabledReason}
      >
        {#snippet overview()}
          <OverviewTab
            {activeInstance}
            installedStats={stats.installedStats}
            playtime={stats.playtime}
            incompatibleCount={stats.incompatibleCount}
            missingModsCount={stats.unresolvedMissing.length}
            running={running !== null}
            {installing}
            {exited}
            {installError}
            {modsError}
            errors={{
              listAccounts: listAccountsError,
              remove: removeError,
              instances: instancesError,
              versions: mcv.error,
            }}
            onManage={() => {
              manageInitialId = null;
              manageOpen = true;
            }}
            onExport={() => (exportDialogOpen = true)}
            onOpenPackDrawer={() => {
              if (activeInstance) modpacksNav.value = { openDrawerForInstance: activeInstance.id };
            }}
            onPackUpdated={() => {
              void refreshInstances();
            }}
            onNavInstalled={() => (modBrowserNav.value = { view: 'installed' })}
            onNavBrowse={() => (modBrowserNav.value = { view: 'browse' })}
            onDismissError={(key) => {
              if (key === 'listAccounts') listAccountsError = null;
              else if (key === 'remove') removeError = null;
              else if (key === 'instances') instancesError = null;
              else if (key === 'versions') mcv.dismissError();
            }}
            onRetryError={(key) => {
              if (key === 'versions') void mcv.load();
            }}
            versionsRetrying={mcv.loading}
            onDismissInstallError={() => (installError = null)}
            onDismissModsError={() => (modsError = null)}
            onOpenLogs={() => {
              logsInitialPath = null;
              logsOpen = true;
            }}
            onOpenServers={() => serversUi.setMode('servers')}
          />
        {/snippet}
      </MainTabs>
    </div>
    <div
      class="col-start-2 row-start-1 overflow-hidden flex flex-col"
      hidden={serversUi.mode !== 'servers'}
    >
      <ServersPanel
        visible={serversUi.mode === 'servers'}
        {instances}
        versions={mcv.value}
        onInstanceCreated={(id) => {
          serversUi.setMode('client');
          void onSelectInstance(id);
        }}
      />
    </div>
  {/if}

  <!--
    Expanded: the footer lives only under the content column (`grid-column: 2`),
    not beneath the full-height sidebar. Compact: single column, so it spans it
    (`1 / -1`). See the sidebar wrapper above; the floor measurement in
    `compact.svelte.ts` mirrors this (status row counted only when compact).
  -->
  <div
    class="row-start-2"
    style="grid-column: {compactState.value ? '1 / -1' : '2'};"
    data-phase-row
  >
    <PhaseStatusRow />
  </div>

  <LogsPopover
    bind:open={logsOpen}
    initialPath={logsInitialPath}
    instanceId={activeInstance?.id ?? null}
    instanceName={activeInstance?.name ?? null}
    mcVersion={activeInstance?.mc_version ?? null}
    loader={activeInstance?.loader ?? null}
    gameRunning={running !== null}
  />

  <ManageInstancesModal
    bind:open={manageOpen}
    bind:instances
    bind:activeInstance
    versions={mcv.value}
    onChanged={refreshInstances}
    isRunning={running !== null}
    initialSelectedId={manageInitialId}
  />

  <InstanceIconDialog onSaved={refreshInstances} />

  <ModpacksModal open={modpacksModalOpen} onClose={() => (modpacksModalOpen = false)}>
    <ModpacksTab
      {instances}
      onImport={(req) =>
        enqueueImport(req.projectId ?? req.path.split(/[\\/]/).pop() ?? 'modpack', req)}
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

  {#if screenshotsGalleryOpen}
    <ScreenshotsGallery onClose={() => (screenshotsGalleryOpen = false)} />
  {/if}
  <!-- SettingsModal renders AFTER ModpacksModal on purpose. Both now use the
       shared Modal primitive, which fixes the backdrop at z-50, so relative
       stacking is decided by DOM order. Settings can be summoned from inside
       the modpacks modal (the CurseForge-key banner), so it must paint on top
       — keeping it last here guarantees that. -->
  <SettingsModal />
  <!-- Page-level operations widget — shows running op + queued ops. Lives outside
       all modals so it survives modal close mid-operation. Renders nothing when idle. -->
  <OperationsView />
  <TourOverlay />
  <QuickJoinDialog
    open={quickJoinOpen}
    {savedServers}
    {savedServersLoading}
    busy={quickJoinBusy}
    connectDisabledReason={quickPlayDisabledReason}
    addDisabledReason={running !== null ? $t('worlds.quickPlay.disabledRunning') : null}
    showOfflineHint={activeAccount?.kind === 'offline'}
    onConnect={(address) => void connectToAddress(address)}
    onSave={onServerSave}
    onSaveAndConnect={onServerSaveAndConnect}
    onDelete={(index, address) => void onServerDelete(index, address)}
    onClose={() => (quickJoinOpen = false)}
  />
  {#if gateReport}
    <PreflightGateDialog
      report={gateReport}
      busy={gateBusy}
      onUpdateLaunch={onGateUpdateLaunch}
      onLaunchAnyway={onGateLaunchAnyway}
      onCancel={onGateCancel}
    />
  {/if}
  {#if exportDialogOpen && activeInstance}
    <ExportPackDialog
      instanceId={activeInstance.id}
      instanceName={activeInstance.name}
      onClose={() => (exportDialogOpen = false)}
    />
  {/if}
  {#if launcherImportOpen}
    <LauncherImportDialog onClose={() => (launcherImportOpen = false)} />
  {/if}
  {#if removeConfirmAccount}
    <RemoveAccountDialog
      accountName={removeConfirmAccount.name}
      onCancel={() => (removeConfirmId = null)}
      onConfirm={confirmRemoveAccount}
    />
  {/if}
  {#if addOfflineOpen}
    <AddOfflineAccountDialog
      error={offlineNameError}
      onCancel={() => {
        addOfflineOpen = false;
        offlineNameError = null;
      }}
      onSubmit={async (name) => {
        offlineNameError = null;
        const result = await commands.addOfflineAccount(name);
        if (result.status === 'ok') {
          await commands.setActiveAccount(result.data.id);
          await refreshAccounts();
          addOfflineOpen = false;
        } else {
          offlineNameError = formatError(result.error);
        }
      }}
    />
  {/if}
  {#if cosmeticsAccount}
    <SkinCapeModal account={cosmeticsAccount} onClose={() => (cosmeticsAccount = null)} />
  {/if}
</main>
<ToastHost />
<MicrosoftSigningInModal
  open={msSigningIn}
  onCancel={() => {
    msSigningIn = false;
  }}
/>

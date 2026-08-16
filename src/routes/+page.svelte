<script lang="ts">
  import {
    commands,
    events,
    type Account,
    type CrashReport,
    type Error as IpcError,
    type InstanceWithStatus,
    type OptimisePlan,
  } from '$lib/ipc/bindings';
  import LogsPopover from '$lib/logs/LogsPopover.svelte';
  import { drainDeferredRepairs } from '$lib/logs/deferred-repairs.svelte';
  import { refreshDiagnosis } from '$lib/logs/log-diagnosis.svelte';
  import { repairCompletionTick } from '$lib/logs/repair-ops.svelte';
  import InstanceIconDialog from '$lib/instances/InstanceIconDialog.svelte';
  import CloneInstanceDialog from '$lib/instances/CloneInstanceDialog.svelte';
  import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';
  import type { ManageFocusField } from '$lib/instances/manage-focus';
  import LocalizationModal from '$lib/l10n/LocalizationModal.svelte';
  import {
    needsStoredKey,
    prefillReadiness,
    type PrefillReadiness,
  } from '$lib/l10n/prefill-readiness';
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
  import { routeDrop } from '$lib/layout/drop-router';
  import { type NavStatusKind } from '$lib/layout/nav-status';
  import { canInstallMods } from '$lib/mods/install-eligibility';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import OverviewTab from '$lib/overview/OverviewTab.svelte';
  import { classifyExit } from '$lib/overview/exit-status';
  import ExportPackDialog from '$lib/modpacks/ExportPackDialog.svelte';
  import LauncherImportDialog from '$lib/instances/import/LauncherImportDialog.svelte';
  import ModpacksTab from '$lib/modpacks/ModpacksTab.svelte';
  import ModpacksModal from '$lib/modpacks/ModpacksModal.svelte';
  import ServersPanel from '$lib/servers/ServersPanel.svelte';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { serversUi } from '$lib/servers/servers-ui.svelte';
  import ScreenshotsGallery from '$lib/screenshots/ScreenshotsGallery.svelte';
  import OperationsBar from '$lib/tasks/OperationsBar.svelte';
  import { installGame } from '$lib/tasks/adapters/game-install';
  import { installModWithDeps } from '$lib/tasks/adapters/mod-install';
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
  import SkinEditorModal from '$lib/accounts/SkinEditorModal.svelte';
  import QuickJoinDialog from '$lib/worlds/QuickJoinDialog.svelte';
  import PreflightGateDialog from '$lib/mods/PreflightGateDialog.svelte';
  import OptimiseDialog from '$lib/mods/OptimiseDialog.svelte';
  import { preflightCache } from '$lib/mods/preflight-cache';
  import { decideLaunch, remediateAll } from '$lib/mods/preflight.svelte';
  import { warningLines } from '$lib/launch/pre-launch-warning';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import type { AppFile_Serialize, PreflightReport, QuickPlay } from '$lib/ipc/bindings';
  import { listen } from '@tauri-apps/api/event';
  import { dispatchIntent } from '$lib/launch/intent';
  import CreateShortcutDialog from '$lib/instances/CreateShortcutDialog.svelte';
  import { classifySignInError } from '$lib/accounts/sign-in-error';
  import { quickPlayDisabledKey } from '$lib/worlds/quick-play-gating';
  import { sweepPings, type PingState } from '$lib/worlds/server-ping';
  import { createQuickWorlds } from '$lib/worlds/quick-worlds.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { initOnboarding, showAccountHint } from '$lib/onboarding/state.svelte';
  import { explanationState } from '$lib/onboarding/explanation-level.svelte';
  import { initTheme } from '$lib/theme/state.svelte';
  import { initLocale } from '$lib/i18n/state.svelte';
  import { t, locale } from '$lib/i18n';
  import { get } from 'svelte/store';
  import { onDestroy, onMount, untrack } from 'svelte';
  import { debounceTrailing } from '$lib/ui/debounce';
  import { SvelteMap } from 'svelte/reactivity';
  import { formatError } from '$lib/ipc/format-error';
  import {
    addonsKind,
    clientActiveTab,
    dragActive,
    droppedAssets,
    droppedMods,
    droppedServerContent,
    droppedWorld,
    modBrowserNav,
    modpacksNav,
    serverAddonsKind,
    serverImportActive,
    settingsOpen,
  } from '$lib/settings/state.svelte';
  import { createMcVersions } from '$lib/versions/mc-versions.svelte';
  import {
    dismiss,
    pushActionToast,
    pushInfo,
    pushSuccess,
    pushWarning,
  } from '$lib/toasts/toasts.svelte';
  import { updateState, runUpdate, dismissUpdate } from '$lib/update/state.svelte';
  import { checkWhatsNew } from '$lib/changelog/whats-new.svelte';
  import WhatsNewModal from '$lib/changelog/WhatsNewModal.svelte';
  import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';
  import { importTitle } from '$lib/modpacks/import-request';
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
  // Standalone skin editor for non-Microsoft / no-account users (Microsoft
  // accounts reach the editor through SkinCapeModal instead). The editor reads
  // the current activeAccount directly, so no separate account state is needed.
  let skinEditorOpen = $state(false);
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
  // False until the FIRST refreshInstances() settles — Sidebar shows a
  // spinner instead of the empty state while this is false.
  let instancesLoaded = $state(false);

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
  // Opened from the Overview Mods card's translation row.
  let l10nOpen = $state(false);
  // Why the AI pre-fill is or is not usable, handed to LocalizationModal as a
  // prop. Both non-ready answers are fixable in Settings, so the modal renders
  // its translate buttons disabled WITH the reason rather than hiding them.
  // Read fresh every time that modal opens rather than once at startup — the
  // same reason refreshPingEnabled re-reads: the permission can be revoked, or
  // a key cleared, in Settings mid-session and the buttons must follow without
  // a restart. The modal itself must NOT read this (its suite pins exact
  // l10nCoverage call counts), and this page already reads settings.
  let l10nAiReady = $state<PrefillReadiness>('no_consent');
  // Which instance LocalizationModal shows. Null = follow the active instance,
  // which is what the Overview row's entry point wants. The Manage modal's
  // entry point sets it, because its selection is independent of the active
  // instance and would otherwise open a different instance's translations
  // than the one whose row the user just clicked.
  let l10nTargetId = $state<string | null>(null);
  $effect(() => {
    if (!l10nOpen) l10nTargetId = null;
  });
  // The instance the modal is actually reporting on, resolved once so the
  // `instanceId` it receives and the Minecraft version handed to its share
  // export can never come from two different instances — the Manage entry
  // point routinely points somewhere other than the active instance, and a
  // shared file's resource-pack half is built for ONE version.
  const l10nInstanceId = $derived(l10nTargetId ?? activeInstance?.id ?? null);
  const l10nInstance = $derived(
    instances.find((i) => i.id === l10nInstanceId) ??
      (activeInstance?.id === l10nInstanceId ? activeInstance : null),
  );

  async function openLocalization() {
    l10nOpen = true;
    const r = await commands.appSettingsGet();
    if (r.status !== 'ok') {
      // Unknown is not permission. Falling back to the most restrictive answer
      // keeps a failed settings read from enabling a paid run.
      l10nAiReady = 'no_consent';
      return;
    }
    const general = r.data.general;
    const consent = general.allow_ai_translation ?? false;
    const provider = general.ai_provider ?? 'anthropic';
    // The keyring is only consulted when its answer could change the outcome —
    // not for a provider that needs no key, and not for a user who has not
    // permitted the feature in the first place.
    const keyStored =
      consent && needsStoredKey(provider)
        ? await commands.l10nPrefillKeyStatus(provider).then((s) => s.status === 'ok' && s.data)
        : false;
    l10nAiReady = prefillReadiness({ consent, provider, keyStored });
  }
  // Which Manage field the modal should scroll to and flash when it opens.
  // Set by whichever entry point opened it; every site that flips manageOpen
  // sets this too, so a stale highlight can never survive into the next open.
  let manageFocus = $state<ManageFocusField | null>(null);
  // Seeds ManageInstancesModal's detail selection. Set to a specific instance id
  // when opened via a per-row "manage this profile" action (the active-instance
  // switch is async, so the modal can't rely on `activeInstance` at open time);
  // null when opened via the generic Manage button (defaults to the active one).
  let manageInitialId = $state<string | null>(null);
  // Source instance for the clone dialog (null = closed). Shared by the
  // sidebar right-click menu and the Manage modal's Clone button.
  let cloneTargetId = $state<string | null>(null);
  let shortcutTargetId = $state<string | null>(null);
  // Whether this OS can write desktop shortcuts at all; the entry points are
  // hidden rather than offering a button that could only ever fail.
  let shortcutSupported = $state(false);
  let msSigningIn = $state(false);
  let exportDialogOpen = $state(false);

  // Per-instance Overview stats (installed-mod counts, incompatible count,
  // playtime, pack-missing mods) live in a dedicated rune composable. The page
  // drives its refreshers from the activeInstance effect, the mod
  // install/uninstall/toggle listeners, and the processExited handler.
  const stats = createInstanceStats();

  let installing = $state(false);
  let installError = $state<string | null>(null);
  // Client (game) processes, keyed by instance_id — multiple instances can run
  // at once. `exited` mirrors it: the LAST exit per instance (crash / exit
  // status + diagnosis), retained per-instance so switching context shows the
  // correct instance's status even while others run or exit.
  let running = new SvelteMap<string, { pid: number; version_id: string }>();
  let exited = new SvelteMap<string, { code: number; user_requested: boolean; log_path: string }>();
  // True when the given instance's game process is up.
  const isRunning = (instanceId: string | undefined): boolean =>
    instanceId !== undefined && running.has(instanceId);
  // Selected-instance projections: the primary Play/Stop button, Overview
  // status, and the props threaded to children are all about the instance
  // currently on screen (the active one), not "any instance running".
  const selectedRunning = $derived(isRunning(activeInstance?.id));
  const selectedExited = $derived(activeInstance ? exited.get(activeInstance.id) : undefined);
  // Client (game) status for the ModeSwitcher's Client segment, so a crash is
  // visible while the user is in Servers mode. Mirrors the servers segment:
  // pulse while running, red after a crash. The red source is `exited` +
  // classifyExit (same lifecycle as the Overview's crash status — clears on
  // relaunch or instance switch), NOT the dismissible `crashReport` banner. A
  // user-requested Stop classifies as `stopped`, never `crashed`. Reflects the
  // SELECTED instance; the aggregate view is the running-instances pill
  // (RunningInstancesPopover), which tracks every process.
  const clientNav = $derived<NavStatusKind>(
    selectedRunning
      ? 'running'
      : selectedExited && classifyExit(selectedExited).kind === 'crashed'
        ? 'crashed'
        : 'idle',
  );

  // One-click Optimise: resolve the curated performance set, preview it, then
  // install the WillInstall entries serially through the existing dep-aware
  // pipeline (mods_install_with_deps emits mod-installed, which refreshes the
  // installed stats).
  let optimiseOpen = $state(false);
  let optimiseResolving = $state(false);
  let optimiseInstalling = $state(false);
  let optimisePlan = $state<OptimisePlan | null>(null);

  async function onOptimise() {
    if (!activeInstance || optimiseResolving) return;
    optimiseResolving = true;
    try {
      const res = await commands.optimiseResolve(
        activeInstance.id,
        activeInstance.mc_version,
        activeInstance.loader,
      );
      if (res.status === 'ok') {
        optimisePlan = res.data;
        optimiseOpen = true;
      } else {
        pushWarning(formatError(res.error));
      }
    } finally {
      optimiseResolving = false;
    }
  }

  async function confirmOptimise() {
    if (!activeInstance || !optimisePlan) return;
    const instanceId = activeInstance.id;
    const toInstall = optimisePlan.entries.filter(
      (e) => e.status.status === 'will_install' && e.version,
    );
    optimiseInstalling = true;
    let installed = 0;
    let failed = 0;
    for (const e of toInstall) {
      if (!e.version) continue;
      const res = await installModWithDeps(instanceId, e.title, e.version, []);
      if (res.status === 'ok') installed += 1;
      else failed += 1;
    }
    optimiseInstalling = false;
    optimiseOpen = false;
    optimisePlan = null;
    void stats.refreshInstalledStats(instanceId);
    const skipped = toInstall.length - installed - failed;
    if (failed > 0) pushWarning($t('optimise.toastSomeFailed', { installed, failed }));
    else pushSuccess($t('optimise.toastDone', { installed, skipped }));
  }

  // Aggregate running-instances pill (ModeSwitcher): the reactive count of live
  // client processes, and an id→name resolver over the page-local instances
  // list. Threaded +page → Sidebar → ModeSwitcher → RunningInstancesPopover.
  const runningCount = $derived(running.size);
  const instanceName = (id: string): string => instances.find((i) => i.id === id)?.name ?? id;
  // Jump from the popover to a running instance: switch to Client mode (the pill
  // is visible in Servers mode too) and select it. Mirrors the ServersPanel
  // "open instance" path below (setMode('client') + onSelectInstance).
  const onOpenRunningInstance = (id: string): void => {
    serversUi.setMode('client');
    void onSelectInstance(id);
  };
  // Tauri event unlisteners, captured so the listeners are torn down on unmount
  // rather than leaking across the page's lifetime. (This is a long-lived
  // single-page shell, but the listeners still need explicit cleanup — an
  // unmounted-then-remounted page would otherwise double-subscribe.)
  let spawnUnlisten: (() => void) | null = null;
  let exitUnlisten: (() => void) | null = null;
  let modInstalledUnlisten: (() => void) | null = null;
  let modUninstalledUnlisten: (() => void) | null = null;
  let modToggleUnlisten: (() => void) | null = null;
  let modsReconciledUnlisten: (() => void) | null = null;
  let intentUnlisten: (() => void) | null = null;
  // initTheme() registers a prefers-color-scheme listener and returns its
  // unlistener. Held (not discarded) because loadStartupSettings is now
  // retryable: a second init would otherwise stack a second matchMedia listener
  // on every retry, and the first one was leaking on unmount already.
  let themeUnlisten: (() => void) | null = null;

  let quickPlaySupported = $state(false);
  // Feeds the Overview Mods card's translation row.
  let l10nPercent = $state<number | null>(null);
  // The target language for translation-coverage checks — shared with
  // LocalizationModal via its bindable `lang` prop so the row and the modal
  // can never silently disagree about which language they're measuring
  // (that mismatch, with the backend defaulting an unset language to
  // "en_us", was the "modal shows 81% but the row says 100%" bug). Seeded
  // from the launcher's already-*resolved* UI locale rather than the raw
  // 'system' preference — the backend has no way to know what "system"
  // resolves to, but the frontend just rendered the whole UI in it.
  let l10nLang = $state($locale ?? 'en');
  // The "needs attention" badge on the Overview Localization row. Deliberately
  // a second source from l10nPercent: percent answers "how much is
  // translated", this answers "is what we have actually live in this
  // instance", and the two refresh on different triggers.
  let l10nBadge = $state<'not_applied' | 'outdated' | null>(null);
  let quickJoinOpen = $state(false);
  let quickJoinBusy = $state(false);
  let savedServers = $state<import('$lib/ipc/bindings').SavedServer[]>([]);
  let savedServersLoading = $state(false);
  // Why the list is empty, when it is empty because the READ failed rather than
  // because there are no servers. `servers_dat_parse` is the case that matters:
  // a truncated or hand-edited servers.dat rendered as "No saved servers yet."
  // and invited the user to re-add servers on top of a file we could not read.
  let savedServersError = $state<string | null>(null);

  async function loadSavedServers() {
    savedServersLoading = true;
    savedServersError = null;
    try {
      if (!activeInstance) {
        savedServers = [];
        return;
      }
      const r = await commands.listSavedServers(activeInstance.id);
      if (r.status === 'ok') {
        savedServers = r.data;
      } else {
        savedServers = [];
        savedServersError = formatError(r.error);
      }
    } finally {
      savedServersLoading = false;
    }
  }

  // Server-status permission (Settings → Game) + per-address results. Read
  // fresh every time the dialog opens rather than cached at startup, so a
  // toggle flipped mid-session takes effect on the next open.
  let pingEnabled = $state(false);
  let pingStates = $state<Record<string, PingState>>({});
  // Sweep generation. Closing the dialog (or starting a newer sweep) makes the
  // running one stop before its next dial, so we honour "only while a server
  // list is open on screen" and two sweeps never interleave their results.
  let pingSweepId = 0;

  async function refreshPingEnabled() {
    const r = await commands.appSettingsGet();
    pingEnabled = r.status === 'ok' ? (r.data.general.allow_server_ping ?? false) : false;
  }

  async function sweepServerPings() {
    if (!pingEnabled) return;
    const sweepId = ++pingSweepId;
    const addresses = savedServers.map((s) => s.address);
    pingStates = Object.fromEntries(addresses.map((a) => [a, 'pending' as const]));
    await sweepPings(
      addresses,
      async (address) => {
        const r = await commands.pingServer(address);
        if (r.status === 'ok') return r.data;
        if (r.error.kind === 'consented_channel_disabled') {
          // Raced with the permission being turned off: fall back to the
          // "status is off" view, which is now the truth.
          pingEnabled = false;
          return null;
        }
        // Anything else is about THIS address, not the permission — an entry
        // that never went through our own add-server validator (hand-edited
        // servers.dat, an IPv6 literal, a modpack override) makes the command
        // reject that one address. Report it as "we could not tell" for that
        // row; blanking the whole list and claiming the setting is off would be
        // a lie about every other server.
        return { kind: 'no_answer' };
      },
      (address, outcome) => {
        // Rebuild the record so the rune sees a new value.
        const next = { ...pingStates };
        if (outcome === null) delete next[address];
        else next[address] = outcome;
        pingStates = next;
      },
      () => quickJoinOpen && pingSweepId === sweepId,
    );
  }

  async function openServersDialog() {
    quickJoinOpen = true;
    pingStates = {};
    await refreshPingEnabled();
    await loadSavedServers();
    // Not awaited: rows appear immediately and fill in as pings land.
    void sweepServerPings();
  }

  // Pre-flight gate: populated when hasBlocking violations are found before launch.
  let gateReport = $state<PreflightReport | null>(null);
  let gateBusy = $state(false);
  // The launch the gate interrupted, so its three buttons resume the RIGHT one.
  // Play, Quick Play and Quick Join all reach the gate, so this can no longer be
  // the single `doLaunch` constant it used to be. Plain state, not `$state`: it
  // is never read from the template.
  let gatePending: { run: () => Promise<void>; onAbort?: () => void } | null = null;
  // True when the dependency pre-flight itself failed, as opposed to finding
  // nothing. It never blocks the launch — the maintainer's decision is that
  // "could not check" marks rather than gates — but it must be visible, because
  // a check that did not run reading as a check that passed is the defect this
  // whole pass exists to remove. Cleared by the next pre-flight that returns a
  // verdict (including a blocking one) and on instance switch.
  //
  // NOT an offline signal: `instance_dependency_preflight` is network-free
  // (pure local jar inspection), so this can only come from an unreadable
  // instance.json or a filesystem error. It is rare by construction today —
  // the value is that the state is representable at all, so the detectors that
  // DO depend on the network can be routed through the same surface later.
  let preflightUnknown = $state(false);

  // Soft pre-launch warning (RAM over-commit / same-account). When launching
  // ANOTHER instance would over-commit memory or reuse a running account,
  // surface a non-blocking confirm before the real launch. `launchWarningLines`
  // holds the human-readable lines to render (null = no dialog); `pendingLaunch`
  // captures the deferred launch so "Launch anyway" can re-dispatch it — mirrors
  // the dependency-preflight gate above (gateReport + onGateLaunchAnyway).
  // `pendingAbort` is an optional cleanup run only if the user CANCELS the
  // warning (e.g. resetting the Quick Join busy flag that was set synchronously).
  let launchWarningLines = $state<string[] | null>(null);
  let pendingLaunch: (() => Promise<void>) | null = null;
  let pendingAbort: (() => void) | null = null;

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

  // The Overview missing-mods indicator and any other deep-link that
  // sets modpacksNav expects the Modpacks view to come up. ModpacksTab
  // itself reads the same rune to flip to the Imported sub-tab.
  $effect(() => {
    if (modpacksNav.value !== null) {
      modpacksModalOpen = true;
    }
  });

  // Keep the persisted server selection honest against the live list. Gated on
  // a SETTLED, SUCCESSFUL load: listLoadedOnce alone isn't enough — a failed
  // load (typed or thrown, both land in listError) must not let reconcile()
  // treat "never really loaded" as "empty" and wipe the persisted selection.
  // listLoading must close the gate too: refresh() clears listError at the
  // START of an attempt, so a Retry after a failed first load would otherwise
  // open the gate against the still-empty list for the in-flight window.
  // Reconcile therefore re-enables only once a retry actually SUCCEEDS.
  $effect(() => {
    if (!serverState.listLoadedOnce || serverState.listLoading || serverState.listError !== null) {
      return;
    }
    serversUi.reconcile(serverState.list.map((s) => s.id));
  });

  // Whenever the active instance changes, clear per-instance error banners.
  // They refer to the previously-active instance and confuse the user when
  // they switch context (e.g. fix one instance's setup by switching to
  // another, only to still see the old error). Exit status is NOT cleared
  // here — it is keyed per-instance (`exited` map), so `selectedExited`
  // already surfaces the newly-active instance's own last exit.
  let lastActiveId: string | null = null;
  $effect(() => {
    const newId = activeInstance?.id ?? null;
    untrack(() => {
      if (newId !== lastActiveId) {
        lastActiveId = newId;
        installError = null;
        modsError = null;
        crashReport = null;
        // Per-instance: the previous instance's pre-flight tells us nothing
        // about this one.
        preflightUnknown = false;
        void stats.refreshInstalledStats(newId);
        void stats.refreshPackStatus(newId);
        void stats.refreshPlaytime(newId);
      }
    });
  });

  // The compatibility scan keys on (instance, mc, loader), so it must be
  // refreshed on a Manage edit too — not only on an instance switch. Without
  // this the Overview kept the count from before the loader change while the
  // Installed tab, whose own effect does track mc/loader, showed the new one.
  // Separate from the effect above precisely because its guard is id-only.
  $effect(() => {
    const inst = activeInstance;
    void inst?.mc_version;
    void inst?.loader;
    untrack(() => void stats.refreshIncompatible(inst?.id ?? null, instances));
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

  // Feeds the Overview Mods card's translation row (l10nPercent), against
  // the shared l10nLang target — see its declaration above.
  $effect(() => {
    const id = activeInstance?.id ?? null;
    const targetLang = l10nLang;
    if (!id) {
      l10nPercent = null;
      return;
    }
    void commands.l10nCoverage(id, targetLang).then((r) => {
      if (activeInstance?.id !== id) return; // ignore stale async result
      if (r.status !== 'ok') {
        l10nPercent = null;
        return;
      }
      l10nPercent = r.data.percent;
      // The backend may resolve a bare launcher locale (e.g. "ru") to a
      // full Minecraft code ("ru_ru"). Write the resolved code back so the
      // row's own label — and LocalizationModal's picker, via the same
      // bindable — converge on it instead of the row being stuck showing
      // the bare guess. A no-op once l10nLang is already a full code.
      if (r.data.lang !== targetLang) l10nLang = r.data.lang;
    });
  });

  // Feeds the same row's "not applied" / "outdated" badge — see l10nBadge's
  // declaration for why this is not folded into the coverage effect above.
  $effect(() => {
    const id = activeInstance?.id ?? null;
    const targetLang = l10nLang;
    // Reading l10nOpen makes this re-run when the translations modal closes.
    // Applying, importing and AI pre-fill all happen with that modal open, so
    // its close is the one cheap signal that any of them may have changed this
    // instance's pack — and there is no point refreshing a badge that is
    // currently behind a modal anyway.
    const modalOpen = l10nOpen;
    if (!id || modalOpen) {
      if (!id) l10nBadge = null;
      return;
    }
    void commands.l10nApplyTargets(targetLang).then((r) => {
      if (activeInstance?.id !== id) return; // ignore a stale async result
      if (r.status !== 'ok') {
        l10nBadge = null;
        return;
      }
      const row = r.data.find((x) => x.instanceId === id);
      l10nBadge = row?.candidate ? (row.state === 'outdated' ? 'outdated' : 'not_applied') : null;
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
      running: selectedRunning,
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
    quickPlaySupported && (activeInstance?.ready ?? false) && !selectedRunning,
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
    // Independent reads — one round-trip instead of two serial ones.
    const [list, active] = await Promise.all([
      commands.listAccounts(),
      commands.getActiveAccount(),
    ]);
    if (list.status === 'ok') {
      accounts = list.data;
    } else {
      listAccountsError = formatError(list.error);
    }
    if (active.status === 'ok') {
      activeAccount = active.data;
    }
  }

  // Keep the compact strip's height synced to its live content (status row
  // appearing/disappearing, etc.). Separate from the async onMount below so
  // Svelte actually uses the returned disposer for cleanup.
  onMount(() => observeCompactContent());

  // The app's single window-level drag-drop listener (moved out of MainTabs
  // when servers mode grew a drop target): +page owns both mode panels, so
  // it owns the window event and routes to the mode-appropriate rune. Its own
  // synchronous onMount — a cleanup returned from the async onMount below
  // would be ignored (see the onDestroy teardown note further down).
  onMount(() => {
    const pendingDrop = getCurrentWebview().onDragDropEvent((event) => {
      if (serverImportActive.value) {
        dragActive.value = false;
        return;
      }
      const payload = (event as { payload: { type: string; paths?: string[] } }).payload;
      const t = payload.type;
      if (t === 'enter' || t === 'over') {
        dragActive.value = true;
        return;
      }
      if (t === 'leave') {
        dragActive.value = false;
        return;
      }
      if (t !== 'drop') return;
      dragActive.value = false;
      const selectedServer = serverState.list.find((s) => s.id === serversUi.selectedServerId);
      const route = routeDrop(payload.paths ?? [], {
        mode: serversUi.mode,
        clientTab: clientActiveTab.value,
        addonsKind: addonsKind.value,
        canInstallMods: canInstallMods(activeInstance?.id ?? null, activeInstance?.loader ?? null),
        instanceSelected: activeInstance !== null,
        serversTab: serversUi.activeTab,
        serverAddonsKind: serverAddonsKind.value,
        serverCanMutate: selectedServer !== undefined && !selectedServer.running,
      });
      if (route === null) return;
      if (route.target === 'client-world') droppedWorld.value = route.paths;
      else if (route.target === 'client-mods') droppedMods.value = route.paths;
      else if (route.target === 'client-assets')
        droppedAssets.value = { kind: route.kind, paths: route.paths };
      else droppedServerContent.value = { kind: route.kind, paths: route.paths };
    });
    return () => {
      void pendingDrop.then((un) => un());
    };
  });

  // Overview-stat refreshes for the mod events registered in onMount below.
  // Trailing-debounced so a multi-jar install burst collapses to one refresh.
  // `force` is required: the compat scan keys on (instance, mc, loader), which
  // a mod change does not alter, so an unforced refresh is deduplicated away
  // and the count never moves.
  const debouncedModSetStats = debounceTrailing(() => {
    void stats.refreshInstalledStats(activeInstance?.id ?? null);
    void stats.refreshIncompatible(activeInstance?.id ?? null, instances, { force: true });
    void stats.refreshPackStatus(activeInstance?.id ?? null);
  }, 150);
  const debouncedModToggleStats = debounceTrailing(() => {
    void stats.refreshInstalledStats(activeInstance?.id ?? null);
    void stats.refreshIncompatible(activeInstance?.id ?? null, instances, { force: true });
  }, 150);
  // Something OTHER than the launcher wrote into an instance's mods/. This page
  // is mounted for the whole session, so it catches the event even when the
  // Installed tab (which has its own, differently-scoped handler) is closed —
  // the common case, since that tab's own mount is what triggers the listing.
  //
  // Deliberately never calls `commands.modsListInstalled`, directly or through a
  // composable: that command is what emits this event, so re-requesting the list
  // here would feed the handler its own trigger. That rules out
  // `stats.refreshInstalledStats`, which lists one level down — the Total /
  // Enabled / Disabled counts are already current without it, because whatever
  // listing produced this event returned the reconciled set to its caller.
  //
  // A single extra listing would terminate on its own (the marker is cleared
  // when taken), but the case this feature exists for is a pack's downloader mod
  // writing dozens of jars over minutes: there, each re-listing finds a FURTHER
  // change, re-arms the marker and re-emits, for as long as the writer runs.
  // `tests/external-change-no-relist.test.ts` pins this.
  const debouncedExternalChangeStats = debounceTrailing(() => {
    void stats.refreshIncompatible(activeInstance?.id ?? null, instances, { force: true });
  }, 150);
  onDestroy(() => {
    debouncedModSetStats.cancel();
    debouncedModToggleStats.cancel();
    debouncedExternalChangeStats.cancel();
  });

  /** Apply one successfully-read settings snapshot to the whole app shell.
   *  Safe to run more than once (the retry path): the theme listener is torn
   *  down before it is re-registered, the modpack sweep is TTL-deduped, and
   *  checkWhatsNew is once-per-version. */
  function applyStartupSettings(data: AppFile_Serialize): void {
    themeUnlisten?.();
    themeUnlisten = initTheme(data.general.theme ?? 'system');
    initLocale(data.general.language ?? 'system');
    explanationState.level = data.general.explanation_level ?? 'basic';
    void initCompact(data.general.compact_mode ?? false);
    initSidebarButtons(data.general.hidden_sidebar_buttons ?? []);
    modpackSweepEnabled = data.general.check_updates_on_startup ?? true;
    sweepModpackUpdates();
    // Post-update "What's new": if the running version differs from the last
    // one the user saw, offer the changelog. Independent of the update-check
    // setting — it's fully offline (embedded changelog, no network).
    // Fire-and-forget; never gate core init on it.
    void checkWhatsNew(data.changelog_seen_version ?? null);

    // Fire-and-forget: this is a best-effort, error-swallowing check, so it
    // must NOT gate core init. Awaiting it here would stall accounts +
    // versions behind the network call (up to the client's 15s connect
    // timeout) on a GitHub outage. Let it resolve on its own; the toast
    // appears whenever it completes.
    if (data.general.check_updates_on_startup) {
      const dismissed = data.update_dismissed_version ?? null;
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
  }

  /** Read app settings and apply them. A failed read used to be swallowed: the
   *  session then silently ran on DEFAULTS — light theme, OS language, expanded
   *  layout, every hidden sidebar button back, update checks on — and the user
   *  had no way to tell that apart from "my settings were reset". Nothing is
   *  written on this path, so the on-disk settings are intact; say so, and offer
   *  the one action that can fix it. Toast + action button is the same shape
   *  `$lib/update/state.svelte.ts` uses for a failed install. */
  async function loadStartupSettings(): Promise<void> {
    const settingsResult = await commands.appSettingsGet();
    if (settingsResult.status === 'ok') {
      applyStartupSettings(settingsResult.data);
      return;
    }
    const tr = get(t);
    pushActionToast(
      'warning',
      tr('page.startupSettings.loadFailed'),
      { label: tr('page.startupSettings.retry'), run: () => void loadStartupSettings() },
      [tr('page.startupSettings.loadFailedDetail'), formatError(settingsResult.error)],
    );
  }

  onMount(async () => {
    void dataLocation.init();
    void refreshInstances();

    // Servers now live in a persistent panel (not a modal), so the store boots
    // with the page: subscribe to events once, then load the list.
    serverState.init();
    void serverState.refresh();

    events.processSpawned
      .listen((event) => {
        const id = event.payload.instance_id;
        running.set(id, { pid: event.payload.pid, version_id: event.payload.version_id });
        // A fresh spawn supersedes any prior exit status for THIS instance.
        exited.delete(id);
      })
      .then((u) => {
        spawnUnlisten = u;
      });

    // Mod-install events refresh the Overview stats so the user can
    // see the Total / Enabled / Disabled numbers tick up after install
    // from the Mod browser without bouncing back through this view.
    // Debounced: a with-deps install emits one event per jar; without
    // coalescing an 8-jar install fired 8 × 3 stat commands in ~2 s.
    events.modInstalled.listen(debouncedModSetStats.call).then((u) => {
      modInstalledUnlisten = u;
    });
    events.modUninstalled.listen(debouncedModSetStats.call).then((u) => {
      modUninstalledUnlisten = u;
    });
    events.modToggle.listen(debouncedModToggleStats.call).then((u) => {
      modToggleUnlisten = u;
    });
    events.modsReconciled
      .listen(({ payload }) => {
        // The pre-flight cache is per-instance, so evicting the key that changed
        // is always right — even for an instance that is not on screen.
        preflightCache.delete(payload.instance_id);
        // The compat scan, in contrast, is ONE shared store holding ONE
        // instance's verdicts. Scanning a background instance into it would
        // replace what the Overview is currently displaying with another
        // instance's data, so the stats only move for the active instance.
        if (payload.instance_id === activeInstance?.id) debouncedExternalChangeStats.call();
      })
      .then((u) => {
        modsReconciledUnlisten = u;
      });

    events.processExited
      .listen(async (event) => {
        const id = event.payload.instance_id;
        running.delete(id);
        exited.set(id, {
          code: event.payload.code,
          user_requested: event.payload.user_requested,
          log_path: event.payload.log_path,
        });
        // Apply any repairs the user queued while the game was running (their
        // files were locked); now the instance is free. Global drain.
        void drainDeferredRepairs();
        // Instance list carries persisted per-instance status → always refresh.
        void refreshInstances();

        // The remaining side-effects drive SINGLE-VALUED, active-instance
        // display state (latest-log diagnosis, Overview playtime, crash
        // banner). Only the instance currently on screen owns those, so a
        // BACKGROUND instance's exit must not refresh/clobber them — the active
        // instance's own data is unchanged by another instance exiting, and
        // each instance's diagnosis/playtime refreshes when it is next selected
        // (the activeInstance effect).
        if (activeInstance?.id !== id) return;

        // A new log was written — refresh the diagnosis indicator.
        void refreshDiagnosis(id);
        void stats.refreshPlaytime(id);
        // A user-requested Stop force-kills the process (non-zero exit code),
        // but it is not a crash — don't surface a crash diagnosis for it.
        if (event.payload.code !== 0 && !event.payload.user_requested) {
          const result = await commands.latestCrash(id);
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

    // Independent of settings — start these before the settings round-trip so
    // the MC version-manifest fetch and the account reads overlap it instead
    // of queueing behind it (cold start used to serialize 4 IPC hops before
    // the manifest fetch even began).
    void mcv.load();
    const accountsReady = refreshAccounts();

    await loadStartupSettings();

    await accountsReady;

    void initOnboarding();

    // Drain once for a cold start (the OS launched us WITH the link/shortcut
    // args), and subscribe for the already-running case, where the
    // single-instance guard parks a second launch's intent and emits this event.
    // Registered after `instances` has loaded so a launch intent can resolve its
    // instance id.
    intentUnlisten = await listen('intent-pending', () => void drainIntent());
    void drainIntent();

    shortcutSupported = await commands.shortcutSupported();
  });

  onDestroy(() => mcv.dispose());

  // Tear down every Tauri event listener registered in onMount. Without this the
  // stored unlisteners were never called and the mod-event listeners were never
  // even captured, so they leaked on unmount.
  onDestroy(() => {
    themeUnlisten?.();
    spawnUnlisten?.();
    exitUnlisten?.();
    modInstalledUnlisten?.();
    modUninstalledUnlisten?.();
    modToggleUnlisten?.();
    modsReconciledUnlisten?.();
    intentUnlisten?.();
  });

  // ── Inbound launch intents (desktop shortcuts, `lucerna://` links) ────────
  // A shortcut's `--launch` and an OS-dispatched link both land in the backend's
  // PendingIntent slot — at cold start from this process's argv, or forwarded by
  // the single-instance guard from a second launch. We PULL (rather than only
  // listening) so a cold-start intent cannot be missed by a listener that was
  // not attached yet; take-once semantics in Rust make the double-drain safe.
  let importUrlPrefill = $state<string | null>(null);
  let importUrlFromExternal = $state(false);

  /** A shortcut asked to launch `instanceId`. Runs the SAME path the Play button
   *  uses, so every gate and every toast is inherited rather than re-implemented:
   *  select the instance, then play (optionally into a world / onto a server). */
  async function launchFromShortcut(instanceId: string, quickPlay: QuickPlay | null) {
    if (!instances.some((i) => i.id === instanceId)) {
      // Renamed or deleted instance: say so plainly instead of failing silently.
      // The shortcut file itself is left alone — it is the user's to delete.
      installError = $t('shortcut.instanceMissing');
      return;
    }
    if (activeInstance?.id !== instanceId) {
      await onSelectInstance(instanceId);
      if (activeInstance?.id !== instanceId) return;
    }
    if (quickPlay === null) {
      await onPlay();
      return;
    }
    if (quickPlay.kind === 'singleplayer') {
      await onQuickPlayWorld(quickPlay.world);
    } else {
      await connectToAddress(quickPlay.address);
    }
  }

  async function drainIntent() {
    const intent = await commands.takePendingIntent();
    await dispatchIntent(intent, {
      onUrl: (url) => {
        // Only ever OPENS the import dialog — a link cannot install or launch.
        importUrlPrefill = url;
        importUrlFromExternal = true;
        modpacksModalOpen = true;
      },
      onLaunch: (instanceId, quickPlay) => launchFromShortcut(instanceId, quickPlay),
    });
  }

  /** Switch the active account. The sidebar Select is fully controlled off
   *  `activeAccount` (Sidebar.svelte: `value={activeAccount?.id ?? ''}`), so a
   *  failed switch leaves the picker showing the OLD account with no visible
   *  difference between "it worked" and "it did not" — and the next Play then
   *  launches as somebody else. A non-active surface has no inline banner to
   *  land in, so this reports the way `onStopInstance` does. */
  async function onSelectAccount(id: string) {
    const result = await commands.setActiveAccount(id);
    if (result.status === 'ok') {
      await refreshAccounts();
    } else {
      pushWarning(get(t)('page.accounts.switchFailed'), [formatError(result.error)]);
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
    // Independent reads — runs on startup AND after every process exit /
    // instance switch, so the serial second hop was paid constantly.
    const [list, active] = await Promise.all([
      commands.listInstances(),
      commands.getActiveInstance(),
    ]);
    if (list.status === 'ok') {
      instances = list.data;
    } else {
      instancesError = formatError(list.error);
      instances = [];
    }
    if (active.status === 'ok') {
      activeInstance = active.data;
    } else {
      activeInstance = null;
    }
    sweepModpackUpdates();
    instancesLoaded = true;
  }

  async function onSelectInstance(id: string) {
    const result = await commands.setActiveInstance(id);
    if (result.status === 'error') {
      instancesError = formatError(result.error);
      return;
    }
    await refreshInstances();
    // The $effect watching activeInstance.id clears per-instance error
    // banners (installError, modsError, crashReport) automatically.
  }

  async function onInstall() {
    if (!activeInstance) return;
    if (activeInstance.mc_version === '') return;
    if (dataLocation.fellBack) return;
    installing = true;
    installError = null;
    const result = await installGame(activeInstance.id, activeInstance.name);
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

  // THE launch chokepoint. Every path that starts the game goes through here,
  // so the dependency pre-flight cannot be skipped by adding a new entry point
  // — which is exactly how Quick Play and Quick Join came to launch with no
  // dependency gate at all while the Play button had one.
  //
  // `run` is the caller's own launch thunk (it owns the QuickPlay target and
  // any per-caller cleanup); `onAbort` runs if the user backs out of EITHER
  // gate. Order: dependency pre-flight first (it can block), then `gateLaunch`
  // for the advisory RAM / account warnings.
  async function startLaunch(run: () => Promise<void>, onAbort?: () => void) {
    if (!activeInstance) return;
    const decision = decideLaunch(await commands.instanceDependencyPreflight(activeInstance.id));
    // Recorded BEFORE the gate branch, not after: `gate` is the outcome that
    // most conclusively proves the check ran, so returning early without
    // clearing the flag would leave "couldn't check dependencies" on screen
    // next to a dialog enumerating the dependency problems it just found.
    preflightUnknown = decision.kind === 'unknown';
    if (decision.kind === 'gate') {
      gateReport = decision.report;
      gatePending = { run, onAbort };
      return;
    }
    await gateLaunch(run, onAbort);
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

    await startLaunch(doLaunch);
  }

  // Gate dialog handlers. Each resumes the launch the gate interrupted — the
  // gate is reachable from Play, Quick Play and Quick Join, so "which launch"
  // is no longer a constant.
  async function onGateLaunchAnyway() {
    const pending = gatePending;
    gateReport = null;
    gatePending = null;
    if (pending) await gateLaunch(pending.run, pending.onAbort);
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
    const pending = gatePending;
    gateReport = null;
    gatePending = null;
    if (pending) await gateLaunch(pending.run, pending.onAbort);
  }

  function onGateCancel() {
    const pending = gatePending;
    gateReport = null;
    gatePending = null;
    // Backing out of the dependency gate must run the caller's cleanup, exactly
    // as backing out of the RAM/account gate does. Quick Join sets its busy flag
    // synchronously before either gate; without this it would stay disabled.
    pending?.onAbort?.();
  }

  // Shared soft-gate for every launch path. Runs preLaunchCheck; when it surfaces
  // warnings (RAM over-commit / same-account), defers `proceed` behind the confirm
  // dialog (with an optional `onAbort` cleanup run only if the user cancels) and
  // returns; otherwise runs `proceed` now. This sits AFTER the dependency
  // preflight, so the deps gate (if any) is resolved first and this is the final
  // advisory check before launch.
  async function gateLaunch(proceed: () => Promise<void>, onAbort?: () => void) {
    if (!activeInstance) return;
    const check = await commands.preLaunchCheck(activeInstance.id);
    if (check.status === 'error') {
      // Fail-open: the RAM/account check is advisory; if it errors, proceed and
      // let launch_instance do the authoritative gating (mirrors the sibling
      // dependency-preflight, which also fails open). Do not surface it as a
      // launch failure.
      console.warn('[+page] preLaunchCheck failed; proceeding without warning', check.error);
      await proceed();
      return;
    }
    const lines = warningLines(check.data);
    if (lines.length === 0) {
      await proceed();
      return;
    }
    pendingLaunch = proceed;
    pendingAbort = onAbort ?? null;
    launchWarningLines = lines;
  }

  function onLaunchWarningConfirm() {
    const proceed = pendingLaunch;
    // The launch proceeds — drop the abort cleanup WITHOUT running it (the
    // proceed thunk owns its own success/reset path).
    pendingLaunch = null;
    pendingAbort = null;
    launchWarningLines = null;
    void proceed?.();
  }

  function onLaunchWarningCancel() {
    const abort = pendingAbort;
    pendingLaunch = null;
    pendingAbort = null;
    launchWarningLines = null;
    // Run the caller's cancel cleanup (e.g. reset the Quick Join busy flag).
    abort?.();
  }

  async function onQuickPlayWorld(folderName: string) {
    if (!activeInstance) return;
    if (!activeAccount) {
      showAccountHint();
      return;
    }
    if (quickPlayDisabledReason !== null) return;
    if (dataLocation.fellBack) return;
    // activeInstance.id is read live inside the thunk; the modal backdrop blocks
    // instance switching while the warning dialog is open, so it can't drift.
    await startLaunch(async () => {
      if (!activeInstance) return;
      installError = null;
      const result = await commands.launchInstance(activeInstance.id, {
        kind: 'singleplayer',
        world: folderName,
      });
      if (result.status === 'error') {
        installError = formatError(result.error);
      }
    });
  }

  async function connectToAddress(address: string) {
    if (!activeInstance) return;
    if (!activeAccount) {
      showAccountHint();
      return;
    }
    if (quickPlayDisabledReason !== null) return;
    if (dataLocation.fellBack) return;
    // Disable the Connect button SYNCHRONOUSLY (before the preLaunchCheck IPC
    // round-trip) so a fast second click can't fire a second launch. The thunk's
    // finally-style reset covers the launch/no-warning/confirm paths; the
    // gateLaunch onAbort resets it if the user CANCELS the warning.
    quickJoinBusy = true;
    // activeInstance.id is read live inside the thunk; the modal backdrop blocks
    // instance switching while the warning dialog is open, so it can't drift.
    await startLaunch(
      async () => {
        if (!activeInstance) return;
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
      },
      () => {
        quickJoinBusy = false;
      },
    );
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
    if (!activeInstance) return;
    const result = await commands.stopInstance(activeInstance.id);
    if (result.status === 'error') {
      installError = formatError(result.error);
    }
  }

  // Stop a SPECIFIC instance by id — the sidebar row's inline Stop (any running
  // instance, not just the active one). Mirrors RunningInstancesPopover.stop:
  // fire the command and surface a failure as a warning toast (a non-active
  // instance's error has no inline banner to land in). On success the
  // processExited event clears the instance's `running` entry, so its row badge
  // and inline Stop disappear reactively.
  async function onStopInstance(id: string) {
    const result = await commands.stopInstance(id);
    if (result.status === 'error') {
      pushWarning(get(t)('sidebar.stop'), [formatError(result.error)]);
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
      {instancesLoaded}
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
      onOpenSkinEditor={() => (skinEditorOpen = true)}
      onAddOffline={() => {
        offlineNameError = null;
        addOfflineOpen = true;
      }}
      {onSelectInstance}
      onOpenManage={() => {
        manageInitialId = null;
        manageFocus = null;
        manageOpen = true;
      }}
      onManageInstance={(id) => {
        manageInitialId = id;
        manageFocus = null;
        manageOpen = true;
      }}
      onCloneInstance={(id) => (cloneTargetId = id)}
      onCreateShortcut={shortcutSupported ? (id) => (shortcutTargetId = id) : undefined}
      {onOpenMods}
      onOpenLogs={() => {
        // Plain "Logs" open is not a deep-link — clear any stale crash path so
        // the viewer selects the newest log instead of re-opening an old crash.
        logsInitialPath = null;
        logsOpen = !logsOpen;
      }}
      running={activeInstance ? (running.get(activeInstance.id) ?? null) : null}
      {clientNav}
      {runningCount}
      {instanceName}
      onOpenInstance={onOpenRunningInstance}
      {isRunning}
      {onStopInstance}
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
       and ServersPanel) stay mounted inside the column; the inactive one gets
       the `hidden` utility CLASS, so component state (active tab, wizard
       progress) survives mode switches — scroll offsets generally don't
       (display:none discards layout). Compact still unmounts the whole column. -->
  {#if !compactState.value}
    <div class="col-start-2 row-start-1 overflow-hidden flex flex-col">
      <!-- System-scoped notice: the data root fallback affects servers too
           (they live under the same data root) — render above both mode panels. -->
      {#if dataLocation.fellBack && dataLocation.status}
        <DataRootFallbackBanner configuredPath={dataLocation.status.configured ?? ''} />
      {/if}
      <!-- class:hidden (not the hidden attr): [hidden] loses the cascade to .flex in Tailwind v3 -->
      <div
        class="overflow-hidden flex flex-col flex-1"
        class:hidden={serversUi.mode !== 'client'}
        data-testid="panel-client"
      >
        <!-- Client-scoped: a game crash refers to the client instance. -->
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
          loaderVersion={activeInstance?.loader_version ?? null}
          onListChanged={() => {
            void refreshInstances();
          }}
          {onQuickPlayWorld}
          {quickPlayDisabledReason}
          running={selectedRunning}
        >
          {#snippet overview()}
            <OverviewTab
              {activeInstance}
              installedStats={stats.installedStats}
              playtime={stats.playtime}
              incompatibleCount={stats.incompatibleCount}
              missingModsCount={stats.unresolvedMissing.length}
              running={selectedRunning}
              {installing}
              exited={selectedExited ?? null}
              {installError}
              {modsError}
              errors={{
                listAccounts: listAccountsError,
                remove: removeError,
                instances: instancesError,
                versions: mcv.error,
              }}
              onManage={(field) => {
                manageInitialId = null;
                manageFocus = field ?? null;
                manageOpen = true;
              }}
              onExport={() => (exportDialogOpen = true)}
              onOpenPackDrawer={() => {
                if (activeInstance)
                  modpacksNav.value = { openDrawerForInstance: activeInstance.id };
              }}
              onPackUpdated={() => {
                void refreshInstances();
              }}
              onNavInstalled={(filter) => (modBrowserNav.value = { view: 'installed', filter })}
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
              {onOptimise}
              {optimiseResolving}
              onOpenLocalization={() => void openLocalization()}
              {l10nPercent}
              {l10nLang}
              {l10nBadge}
              {preflightUnknown}
            />
          {/snippet}
        </MainTabs>
      </div>
      <div
        class="overflow-hidden flex flex-col flex-1"
        class:hidden={serversUi.mode !== 'servers'}
        data-testid="panel-servers"
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
    </div>
  {/if}

  <LogsPopover
    bind:open={logsOpen}
    initialPath={logsInitialPath}
    instanceId={activeInstance?.id ?? null}
    instanceName={activeInstance?.name ?? null}
    mcVersion={activeInstance?.mc_version ?? null}
    loader={activeInstance?.loader ?? null}
    gameRunning={selectedRunning}
  />

  <ManageInstancesModal
    bind:open={manageOpen}
    bind:instances
    bind:activeInstance
    versions={mcv.value}
    onChanged={refreshInstances}
    isRunning={selectedRunning}
    initialSelectedId={manageInitialId}
    focusField={manageFocus}
    onCloneRequest={(id) => (cloneTargetId = id)}
    onShortcutRequest={shortcutSupported ? (id) => (shortcutTargetId = id) : undefined}
    onTranslationsRequest={(id) => {
      l10nTargetId = id;
      void openLocalization();
    }}
  />

  <LocalizationModal
    bind:open={l10nOpen}
    bind:lang={l10nLang}
    instanceId={l10nInstanceId}
    mcVersion={l10nInstance?.mc_version ?? ''}
    aiReady={l10nAiReady}
  />

  {#if cloneTargetId !== null}
    {@const cloneSource = instances.find((i) => i.id === cloneTargetId)}
    {#if cloneSource}
      <CloneInstanceDialog instance={cloneSource} onClose={() => (cloneTargetId = null)} />
    {/if}
  {/if}

  {#if shortcutTargetId !== null}
    {@const shortcutSource = instances.find((i) => i.id === shortcutTargetId)}
    {#if shortcutSource}
      <CreateShortcutDialog instance={shortcutSource} onClose={() => (shortcutTargetId = null)} />
    {/if}
  {/if}

  <InstanceIconDialog onSaved={refreshInstances} />

  <ModpacksModal open={modpacksModalOpen} onClose={() => (modpacksModalOpen = false)}>
    <ModpacksTab
      {instances}
      urlPrefill={importUrlPrefill}
      urlFromExternal={importUrlFromExternal}
      onUrlConsumed={() => {
        importUrlPrefill = null;
        importUrlFromExternal = false;
      }}
      onImport={(req) => enqueueImport(importTitle(req), req)}
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
  <!--
    The operations strip — collapsed bottom strip + its expandable panel/report
    modal (`$lib/tasks/OperationsBar.svelte`). Grid placement matches the old
    `PhaseStatusRow` mount it replaces (row 2; expanded: content column only —
    `grid-column: 2` — so it never steals height from the full-height sidebar;
    compact: single column, so it spans `1 / -1`. See `compact.svelte.ts`,
    which measures `[data-phase-row]`'s height for the compact window floor).
    CSS Grid placement does not depend on DOM order, so mounting it here (AFTER
    SettingsModal) is safe for layout — and required for stacking: the panel's
    disclosure popover and the report modal both paint at the same z-index
    tier as a Modal backdrop (`--z-popover` == Modal's `z-50`), so whichever
    paints LATER in the DOM wins ties. Mounted before SettingsModal, the panel
    would paint underneath any modal that happens to be open; mounted here, it
    always paints on top — matching the retired `OperationsView` corner
    widget's "outside all modals" placement. Renders nothing when idle.
  -->
  <div
    class="row-start-2"
    style="grid-column: {compactState.value ? '1 / -1' : '2'};"
    data-phase-row
  >
    <OperationsBar />
  </div>
  <TourOverlay />
  <QuickJoinDialog
    open={quickJoinOpen}
    {savedServers}
    {savedServersLoading}
    {savedServersError}
    busy={quickJoinBusy}
    connectDisabledReason={quickPlayDisabledReason}
    addDisabledReason={selectedRunning ? $t('worlds.quickPlay.disabledRunning') : null}
    showOfflineHint={activeAccount?.kind === 'offline'}
    {pingEnabled}
    {pingStates}
    onRefreshPings={() => void sweepServerPings()}
    onOpenPingSetting={() => {
      quickJoinOpen = false;
      settingsOpen.value = { tab: 'game' };
    }}
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
  {#if launchWarningLines}
    <ConfirmDialog
      title={$t('launch.warning.title')}
      bodyText={launchWarningLines}
      confirmLabel={$t('launch.warning.launchAnyway')}
      confirmTestid="launch-warning-confirm"
      onCancel={onLaunchWarningCancel}
      onConfirm={onLaunchWarningConfirm}
    />
  {/if}
  {#if exportDialogOpen && activeInstance}
    <ExportPackDialog
      instanceId={activeInstance.id}
      instanceName={activeInstance.name}
      onClose={() => (exportDialogOpen = false)}
    />
  {/if}
  {#if optimiseOpen && optimisePlan && activeInstance}
    <OptimiseDialog
      plan={optimisePlan}
      loader={activeInstance.loader}
      mc={activeInstance.mc_version}
      installing={optimiseInstalling}
      onConfirm={confirmOptimise}
      onCancel={() => {
        if (!optimiseInstalling) {
          optimiseOpen = false;
          optimisePlan = null;
        }
      }}
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
  {#if skinEditorOpen}
    <SkinEditorModal
      account={activeAccount}
      initialSkinB64={null}
      initialVariant="classic"
      onClose={() => (skinEditorOpen = false)}
    />
  {/if}
</main>
<ToastHost />
<WhatsNewModal />
<MicrosoftSigningInModal
  open={msSigningIn}
  onCancel={() => {
    msSigningIn = false;
  }}
/>

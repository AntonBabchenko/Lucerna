<script lang="ts">
  import type { Account, InstanceWithStatus, WorldQuickEntry } from '$lib/ipc/bindings';
  import PlayWithWorlds from '$lib/layout/PlayWithWorlds.svelte';
  import InstanceAvatar from '$lib/instances/InstanceAvatar.svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';
  import { diagnosisStatus } from '$lib/logs/log-diagnosis.svelte';
  import InstanceConceptTooltip from '$lib/onboarding/InstanceConceptTooltip.svelte';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import { settingsOpen } from '$lib/settings/state.svelte';
  import MicrosoftSignInButton from '$lib/accounts/MicrosoftSignInButton.svelte';
  import PlayerHead from '$lib/accounts/PlayerHead.svelte';
  import Select from '$lib/ui/Select.svelte';
  import type { SelectOption } from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { focusInstancePickerIfLost } from './picker-focus';
  import { Icon } from '$lib/ui/icons';
  import { rainbowFx } from '$lib/fx/rainbow-fx.svelte';
  import { t } from '$lib/i18n';
  import { tooltip } from '$lib/ui/tooltip';
  import { validateOfflineName } from '$lib/accounts/offline-name';
  import { serversUi } from '$lib/servers/servers-ui.svelte';
  import ServerSidebarSection from '$lib/servers/ServerSidebarSection.svelte';
  import ModeSwitcher from '$lib/layout/ModeSwitcher.svelte';
  import { navVisual, type NavStatusKind } from '$lib/layout/nav-status';
  import NavStatusIcon from '$lib/layout/NavStatusIcon.svelte';
  import NavFixWrench from '$lib/layout/NavFixWrench.svelte';
  import { isVisible, setHidden } from '$lib/layout/sidebar-buttons.svelte';
  import ContextMenu, { type ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';
  import { buildInstanceMenuItems } from '$lib/instances/instance-menu';
  import CountPill from '$lib/ui/cards/CountPill.svelte';
  import Menu from '$lib/ui/Menu.svelte';
  import { estimateMenuHeight } from '$lib/ui/menu-metrics';
  import HideButtonConfirmDialog from '$lib/layout/HideButtonConfirmDialog.svelte';
  import AccountRequiredDialog from '$lib/layout/AccountRequiredDialog.svelte';
  import { SIDEBAR_BUTTONS, type SidebarButtonId } from '$lib/layout/sidebar-buttons';
  import SidebarSection from '$lib/layout/SidebarSection.svelte';
  import HideableSidebarButton from '$lib/layout/HideableSidebarButton.svelte';
  import { summarizeIntegrity } from '$lib/instances/integrity-summary';

  let {
    accounts,
    activeAccount,
    instances,
    activeInstance,
    onSelectAccount,
    onRemoveAccount,
    onOpenCosmetics,
    onAddOffline,
    onSelectInstance,
    onCreateInstance,
    onOpenInstanceSettings,
    onActivateInstance,
    onRenameInstance,
    onCloneInstance = () => {},
    onCreateShortcut,
    onTranslationsInstance,
    onExportInstance,
    onOpenInstanceFolder,
    onOpenInstanceModsFolder,
    onDeleteInstance,
    isBusy = () => false,
    onOpenMods,
    onOpenLogs,
    onOpenModpacks,
    onOpenLauncherImport,
    running,
    clientNav = 'idle',
    runningCount = 0,
    instanceName = (id: string) => id,
    onOpenInstance = () => {},
    isRunning = () => false,
    onStopInstance = () => {},
    installing,
    onPlay,
    onStop,
    onInstall,
    worlds = [],
    onQuickPlayWorld = () => {},
    quickPlayMenuEnabled = false,
    msSigningIn = $bindable(false),
    onMicrosoftSignedIn,
    onMicrosoftError,
    compact = false,
    onToggleCompact = () => {},
    onOpenQuickJoin = () => {},
    onOpenGallery = () => {},
    onOpenSkinEditor = () => {},
    playBlockedReason = null,
    createBlockedReason = null,
    launcherImportBlockedReason = null,
    dataRootFallbackReason = null,
    instancesLoaded = true,
    instancesListError = null,
    onRetryInstances = () => {},
  }: {
    accounts: Account[];
    activeAccount: Account | null;
    instances: InstanceWithStatus[];
    activeInstance: InstanceWithStatus | null;
    onSelectAccount: (id: string) => void;
    onRemoveAccount: (id: string) => void;
    onOpenCosmetics: (account: Account) => void;
    // Open the add-offline-account dialog. The name entry + the actual
    // addOfflineAccount call live in that modal / the page handler, not here:
    // the inline field this used to reveal was too easy to miss.
    onAddOffline: () => void;
    onSelectInstance: (id: string) => void;
    /** Open the create-instance dialog (New instance, and the empty state). */
    onCreateInstance: () => void;
    /** Open the Settings tab for this instance — the row's settings icon. Its
     *  mousedown also commits the row, which makes the instance active; the
     *  page's request waits for that activation. */
    onOpenInstanceSettings?: (instanceId: string) => void;
    // The row menu's actions (shared builder, instance-menu.ts). Each one left
    // undefined omits its item — a bare mount offers only what it can do.
    onActivateInstance?: (instanceId: string) => void;
    onRenameInstance?: (instanceId: string) => void;
    // Open the clone dialog for a specific profile. Defaults to a no-op.
    onCloneInstance?: (instanceId: string) => void;
    /** Undefined on platforms without desktop-shortcut support — the menu item
     *  is then omitted rather than shown disabled. */
    onCreateShortcut?: (instanceId: string) => void;
    onTranslationsInstance?: (instanceId: string) => void;
    onExportInstance?: (instanceId: string) => void;
    onOpenInstanceFolder?: (instanceId: string) => void;
    onOpenInstanceModsFolder?: (instanceId: string) => void;
    onDeleteInstance?: (instanceId: string) => void;
    /** A task scoped to this instance is running or queued (install, verify,
     *  mod install, pack update, world migration): Delete waits for it. */
    isBusy?: (instanceId: string) => boolean;
    onOpenMods: () => void;
    onOpenLogs: () => void;
    // Open the global Modpacks browser (a full-screen modal). Modpacks aren't
    // tied to the selected instance — installing a pack creates a new one — so
    // the entry point lives at the sidebar level, not as a per-instance tab.
    onOpenModpacks: () => void;
    // Open the launcher-instance import dialog (step 1: discover/browse for
    // existing launcher instances; step 2: pick content + name).
    onOpenLauncherImport: () => void;
    // Launch-state inputs (moved here from the Overview pane in
    // +page.svelte). running !== null = MC is up; installing = an
    // install pipeline is in flight; otherwise the button morphs
    // between Install (not-ready) and Play (ready).
    running: { version_id: string; pid: number } | null;
    // Client (game) status for the ModeSwitcher's Client segment, derived in
    // +page.svelte from running/exited and threaded through as an opaque enum.
    clientNav?: NavStatusKind;
    // Aggregate running-instances pill inputs, threaded straight through to
    // ModeSwitcher (Sidebar is only the conduit — client game state stays
    // page-local in +page.svelte). runningCount is the page's reactive
    // `running.size`; instanceName resolves an id to its display name; and
    // onOpenInstance jumps to a running instance (select + Client mode).
    runningCount?: number;
    instanceName?: (id: string) => string;
    onOpenInstance?: (id: string) => void;
    // Per-instance running state for the sidebar's instance rows. `isRunning`
    // mirrors the `instanceName` function-prop: it reads the page's reactive
    // `running` SvelteMap (running.has(id)) across the component boundary, so a
    // row's badge appears / disappears live as instances spawn / exit.
    // `onStopInstance` stops a specific instance by id (mirrors
    // RunningInstancesPopover's Stop — commands.stopInstance + a toast on error,
    // implemented in +page.svelte). Defaults keep no-prop mounts (tests)
    // rendering unchanged: nothing is ever "running".
    isRunning?: (id: string) => boolean;
    onStopInstance?: (id: string) => void;
    installing: boolean;
    onPlay: () => void;
    onStop: () => void;
    onInstall: () => void;
    worlds?: WorldQuickEntry[];
    onQuickPlayWorld?: (folderName: string) => void;
    quickPlayMenuEnabled?: boolean;
    msSigningIn?: boolean;
    onMicrosoftSignedIn?: (account: unknown) => void;
    onMicrosoftError?: (err: unknown) => void;
    compact?: boolean;
    onToggleCompact?: () => void;
    onOpenQuickJoin?: () => void;
    onOpenGallery?: () => void;
    onOpenSkinEditor?: (account: Account | null) => void;
    // Non-null while the configured data root is unavailable (§7 fallback
    // gating): disables Play/Install (with an explanatory tooltip), the
    // empty-state "+ Create" shortcut and New instance. See data-root-gating.ts.
    playBlockedReason?: string | null;
    createBlockedReason?: string | null;
    // Non-null while the data root is unavailable: disables the "Import from
    // launcher" entry point (importing creates a new instance) — see
    // data-root-gating.ts.
    launcherImportBlockedReason?: string | null;
    // Non-null while the data root is unavailable: renders the same
    // non-dismissible fallback notice DataRootFallbackBanner shows in the
    // content column, but inline in the sidebar. In expanded mode the page
    // already renders the full banner in the content column, so this is
    // effectively compact-mode-only (see +page.svelte), but it's driven purely
    // by this prop so Sidebar itself doesn't need to know about compact mode.
    dataRootFallbackReason?: string | null;
    // False until the page's first list_instances response settles. While
    // false and the list is empty, the sidebar shows a spinner instead of
    // the "No instances yet" empty state — "not loaded yet" must not read
    // as "you have no instances" (it did, for seconds, on cold starts).
    // Defaults to true so mounts that never load asynchronously (tests)
    // keep the old semantics.
    instancesLoaded?: boolean;
    /** The instance list could not be read: «could not tell», never shown as «no instances». */
    instancesListError?: string | null;
    onRetryInstances?: () => Promise<unknown> | void;
  } = $props();

  const accountOptions = $derived(
    accounts.map((a) => ({
      value: a.id,
      label: `${a.name} (${
        a.kind === 'microsoft'
          ? $t('sidebar.accountKindMicrosoft')
          : $t('sidebar.accountKindOffline')
      })`,
    })),
  );
  let retryingInstances = $state(false);
  async function retryInstances() {
    if (retryingInstances) return;
    retryingInstances = true;
    try {
      await onRetryInstances();
    } finally {
      retryingInstances = false;
    }
    await focusInstancePickerIfLost();
  }

  const instanceOptions = $derived(
    instances.map((i) => ({
      value: i.id,
      label: `${i.name} · ${displayLoader(i.loader)} ${i.mc_version || $t('sidebar.pickMcVersion')}`,
    })),
  );
  // The Manage window's list offered a name filter above this many instances (#510 removed
  // it); the dropdown, now the only instance list, offers one from the same point — about
  // where its 240 px list starts to scroll.
  const INSTANCE_FILTER_THRESHOLD = 8;
  // By the name only: every row also reads «· Fabric 1.21.1», so «f» would match every
  // Fabric and Forge instance.
  const instanceFilter = $derived(
    instances.length > INSTANCE_FILTER_THRESHOLD
      ? {
          placeholder: $t('sidebar.instanceFilterPlaceholder'),
          ariaLabel: $t('sidebar.instanceFilterAriaLabel'),
          noMatches: (query: string) => $t('sidebar.instanceFilterNoMatches', { query }),
          text: (opt: SelectOption) => instances.find((i) => i.id === opt.value)?.name ?? opt.label,
        }
      : undefined,
  );

  const logsNav: NavStatusKind = $derived(
    diagnosisStatus() === 'actionable'
      ? 'actionable'
      : diagnosisStatus() === 'advisory'
        ? 'advisory'
        : 'idle',
  );
  const logsVisual = $derived(navVisual(logsNav));
  const logsStatusLabel = $derived(logsNav === 'advisory' ? $t('sidebar.logsAdvisory') : null);

  let hideCandidate = $state<SidebarButtonId | null>(null);
  // Set when the user tries to hide the account-add buttons with no account yet.
  // Those buttons are force-shown while account-less (dead-end guard); hiding is
  // refused here with an explanation instead of the usual hide-confirm.
  let accountRequiredOpen = $state(false);

  // Route a hide request from a button's right-click menu. Hiding the
  // account-add buttons while there are no accounts would trap the user (no way
  // to sign in, so no way to launch), so intercept that case and explain.
  function requestHide(id: SidebarButtonId): void {
    if (id === 'account_actions' && accounts.length === 0) {
      accountRequiredOpen = true;
    } else {
      hideCandidate = id;
    }
  }

  function hideMenuItems(id: SidebarButtonId): ContextMenuItem[] {
    return [
      {
        label: $t('sidebar.contextHide'),
        icon: 'eyeOff',
        testId: `sidebar-ctx-hide-${id}`,
        onSelect: () => requestHide(id),
      },
    ];
  }

  function hideCandidateLabel(): string {
    const b = SIDEBAR_BUTTONS.find((x) => x.id === hideCandidate);
    return b ? $t(b.labelKey) : '';
  }

  // Right-click menu for a row in the profile dropdown (opened via the
  // Select's onOptionContextMenu hook; rendered with the shared Menu).
  //
  // Everything the menu shows is computed ONCE, when it opens, and stored
  // here: Menu calls onClose() — which nulls `instanceMenu` — BEFORE
  // onSelect(), so nothing reached at select time may read this state (a
  // template `{@const}` re-derives at call time and crashes on null). The
  // builder's handlers close over the plain id for the same reason.
  let instanceMenu = $state.raw<{
    top: number;
    left: number;
    label: string;
    items: ContextMenuItem[];
    openedByKeyboard: boolean;
    returnFocusEl: HTMLElement | null;
  } | null>(null);

  // Where the menu may sit — the ContextMenu.openAt rule: inside the window,
  // MENU_MARGIN off its edges, by its width and its estimated height.
  const MENU_WIDTH = 220;
  const MENU_MARGIN = 8;

  function openInstanceMenu(
    id: string,
    pos: { x: number; y: number },
    how: { byKeyboard: boolean },
  ) {
    const target = instances.find((i) => i.id === id);
    // The one builder the Settings tab's Delete shares (instance-menu.ts), with
    // every action this surface hosts. Create-shortcut is omitted entirely
    // where the OS has no shortcut support (the page leaves the handler
    // undefined) rather than shown disabled.
    const items = buildInstanceMenuItems({
      // Unknown instance → 'vanilla', the answer that offers less.
      instance: { id, loader: target?.loader ?? 'vanilla' },
      isActive: id === activeInstance?.id,
      isRunning: isRunning(id),
      isBusy: isBusy(id),
      isLast: instances.length <= 1,
      testIdPrefix: 'sidebar-ctx',
      t: $t,
      handlers: {
        onActivate: onActivateInstance,
        onRename: onRenameInstance,
        onClone: onCloneInstance,
        onShortcut: onCreateShortcut,
        onTranslations: onTranslationsInstance,
        onExport: onExportInstance,
        onOpenFolder: onOpenInstanceFolder,
        onOpenModsFolder: onOpenInstanceModsFolder,
        onDelete: onDeleteInstance,
      },
    });
    instanceMenu = {
      top: Math.min(
        Math.max(pos.y, MENU_MARGIN),
        Math.max(MENU_MARGIN, window.innerHeight - estimateMenuHeight(items) - MENU_MARGIN),
      ),
      left: Math.min(
        Math.max(pos.x, MENU_MARGIN),
        Math.max(MENU_MARGIN, window.innerWidth - MENU_WIDTH - MENU_MARGIN),
      ),
      label: $t('instance.menu.aria', { name: target?.name ?? id }),
      items,
      openedByKeyboard: how.byKeyboard,
      // From the keyboard, Select has closed its list and refocused its
      // trigger before calling; from the mouse, focus is wherever it was.
      returnFocusEl: document.activeElement as HTMLElement | null,
    };
  }

  // Menu does not return focus itself (its trigger wrappers do). Back to where
  // the menu was opened from — unless the chosen action moved focus on purpose
  // (Rename puts it in the Settings tab's name field): only a focus that fell
  // to <body> is brought back.
  function closeInstanceMenu() {
    const back = instanceMenu?.returnFocusEl ?? null;
    instanceMenu = null;
    queueMicrotask(() => {
      if (document.activeElement === document.body || document.activeElement === null) {
        back?.focus?.();
      }
    });
  }
</script>

<aside data-sidebar class="h-full bg-base border-r border-border-subtle p-3 overflow-y-auto">
  <!--
    Non-dismissible fallback notice (§7): only passed by the page in compact
    mode, since expanded mode already renders DataRootFallbackBanner in the
    content column. Placed INSIDE <aside> (ahead of data-sidebar-content) so
    compact.svelte.ts's height measurement — anchored to <aside>'s own top and
    data-sidebar-content's bottom — picks it up automatically; a sibling
    outside <aside> would be invisible to that measurement and get clipped.
  -->
  {#if dataRootFallbackReason}
    <div
      class="mb-3 flex items-start gap-2 rounded-md bg-warning-bg border border-warning-text px-3 py-2 text-xs text-warning-text"
      data-testid="data-root-fallback-banner-compact"
      role="alert"
    >
      <Icon name="warning" size={14} class="mt-0.5 shrink-0" />
      <span>{dataRootFallbackReason}</span>
    </div>
  {/if}
  <!--
    Content wrapper: its box height equals the sidebar's CONTENT height (the
    <aside> is `h-full`, so its own box tracks the window, not the content).
    `compact.svelte.ts` observes `[data-sidebar-content]` so a content reflow
    (e.g. a live locale switch changing button-label sizes) re-applies the
    window's min-height floor.
  -->
  <div data-sidebar-content class="flex flex-col gap-3">
    <div class="flex items-center justify-between">
      <span class="font-bold text-lg text-primary">Lucerna</span>
      <button
        type="button"
        class="btn-icon btn-icon-sm"
        aria-label={compact ? $t('sidebar.compactExpand') : $t('sidebar.compactCollapse')}
        use:tooltip={compact ? $t('sidebar.compactExpand') : $t('sidebar.compactCollapse')}
        onclick={onToggleCompact}
      >
        <Icon name={compact ? 'expand' : 'shrink'} size={14} />
      </button>
    </div>

    <ModeSwitcher {clientNav} {runningCount} {instanceName} {onOpenInstance} />

    <!--
      Account is a purely client-side concept: which player identity launches
      the game and owns skins/capes. Hosting a server needs no account, so the
      account section shares the instance section's client-mode gate below —
      servers mode shows only ServerSidebarSection. The onboarding tour +
      account-hint anchor on [data-tour="account-section"], so state.svelte.ts
      forces client mode before showing them (initOnboarding / showAccountHint).
    -->
    {#if serversUi.mode === 'client'}
      <SidebarSection heading={$t('sidebar.account')} dataTour="account-section">
        {#if accounts.length === 0}
          <!-- In a recovery session the list is empty because the data folder is unavailable —
               "no accounts yet" would read as data loss. -->
          <p class="text-xs text-muted">
            {$t(dataLocation.fellBack ? 'sidebar.noAccountsFallback' : 'sidebar.noAccounts')}
          </p>
        {:else}
          {#snippet accountLeading(opt: SelectOption)}
            {@const acc = accounts.find((a) => a.id === opt.value)}
            {#if acc}
              <PlayerHead uuid={acc.uuid} name={acc.name} size={20} />
              {#if acc.kind === 'offline' && validateOfflineName(acc.name) !== null}
                <span
                  class="text-warning-text flex-shrink-0"
                  use:tooltip={{ text: $t('sidebar.offlineNameUnsupported'), describe: false }}
                >
                  <Icon name="warning" size={14} />
                </span>
              {/if}
            {/if}
          {/snippet}
          <!-- Per-row trash inside the open dropdown: always visible, neutral at
           rest, red on hover/focus (btn-icon-danger, §6 delete-icon model).
           Removes that specific account (gated by the confirm dialog in
           +page.svelte). onmousedown is stopped so clicking the trash does not
           also commit/select the row; Delete on the active row routes through
           Select's onDeleteOption. -->
          {#snippet accountTrailing(opt: SelectOption)}
            {@const acc = accounts.find((a) => a.id === opt.value)}
            {#if acc}
              {@const removeLabel = $t('sidebar.removeAccountLabel', { name: acc.name })}
              <button
                type="button"
                tabindex="-1"
                class="btn-icon btn-icon-sm btn-icon-danger flex-shrink-0"
                aria-label={removeLabel}
                use:tooltip={{ text: removeLabel, describe: false }}
                onmousedown={(e) => {
                  e.stopPropagation();
                  e.preventDefault();
                }}
                onclick={() => onRemoveAccount(acc.id)}
              >
                <Icon name="trash" size={14} />
              </button>
            {/if}
          {/snippet}
          <Select
            class="w-full text-sm"
            value={activeAccount?.id ?? ''}
            options={accountOptions}
            onChange={(v) => onSelectAccount(String(v))}
            ariaLabel={$t('sidebar.account')}
            optionLeading={accountLeading}
            valueLeading={accountLeading}
            optionTrailing={accountTrailing}
            onDeleteOption={(opt) => onRemoveAccount(String(opt.value))}
          />
        {/if}
        <!--
        Skin entry point — one adaptive button in the account action cluster.
        A Microsoft account opens the full Skin & cape modal (server-side
        cosmetics + the editor); everyone else (offline or no account) opens the
        standalone pixel skin editor directly. Only uploading a skin to Mojang
        needs a Microsoft login. Hideable like the other secondary buttons
        (right-click → hide, or Settings → Appearance): it is a convenience
        entry point, not required to launch, so hiding it traps nobody.
      -->
        <HideableSidebarButton
          id="skin"
          hideItems={hideMenuItems('skin')}
          contextMenuAria={$t('sidebar.contextMenuAria')}
          testid="sidebar-open-skin"
          onclick={() =>
            activeAccount?.kind === 'microsoft'
              ? onOpenCosmetics(activeAccount)
              : onOpenSkinEditor(activeAccount)}
        >
          <Icon name={activeAccount?.kind === 'microsoft' ? 'shirt' : 'edit'} size={14} />
          {activeAccount?.kind === 'microsoft' ? $t('cosmetics.title') : $t('skinEditor.open')}
        </HideableSidebarButton>
        <!--
        Force the account-add buttons visible when there are no accounts, even
        if the user hid `account_actions` in Settings — otherwise an
        account-less launcher is a dead end: the empty-state text says "add one
        below" but there is nothing to add with, and there is no way to sign in.
        Once an account exists the hidden preference is honoured again.
      -->
        {#if isVisible('account_actions') || accounts.length === 0}
          <ContextMenu
            items={hideMenuItems('account_actions')}
            ariaLabel={$t('sidebar.contextMenuAria')}
          >
            <button
              type="button"
              class="btn-secondary btn-xs w-full flex items-center justify-center gap-1"
              onclick={() => onAddOffline()}
            >
              <Icon name="userPlus" size={14} />
              {$t('sidebar.addOffline')}
            </button>
            <div class="mt-2">
              <MicrosoftSignInButton
                bind:signingIn={msSigningIn}
                onSignedIn={(account) => onMicrosoftSignedIn?.(account)}
                onError={(err) => onMicrosoftError?.(err)}
              />
            </div>
          </ContextMenu>
        {/if}
      </SidebarSection>

      <SidebarSection>
        <div class="text-xs uppercase tracking-wide text-muted flex items-center gap-1">
          <span>{$t('sidebar.instance')}</span>
          <InstanceConceptTooltip />
        </div>
        {#if !instancesLoaded && instances.length === 0}
          <div class="py-1" data-testid="sidebar-instances-loading">
            <Spinner size="sm" delayMs={150} class="text-muted" />
          </div>
        {:else if instances.length === 0 && instancesListError}
          <div class="flex flex-col items-start gap-1" data-testid="sidebar-instances-failed">
            <StatusMessage
              tone="danger"
              message={`${$t('sidebar.instancesFailed')} ${instancesListError}`}
              class="text-xs"
            />
            <!-- Never disabled while it runs (the focus would fall to <body>): busy, then the
                 focus goes to the instance list it filled, or stays here if it failed again. -->
            <button
              type="button"
              class="btn-secondary btn-xs inline-flex items-center gap-1"
              aria-disabled={retryingInstances}
              aria-busy={retryingInstances}
              onclick={() => void retryInstances()}
            >
              {#if retryingInstances}<Spinner size="sm" />{/if}
              {$t('sidebar.instancesRetry')}
            </button>
          </div>
        {:else if instances.length === 0}
          <p class="text-xs text-muted">
            {$t(dataLocation.fellBack ? 'sidebar.noInstancesFallback' : 'sidebar.noInstances')}
          </p>
          {#if createBlockedReason}
            <span class="inline-flex" use:tooltip={{ text: createBlockedReason, describe: false }}>
              <button type="button" class="btn-primary btn-xs" disabled>
                {$t('sidebar.createInstance')}
              </button>
            </span>
          {:else}
            <button type="button" class="btn-primary btn-xs" onclick={onCreateInstance}>
              {$t('sidebar.createInstance')}
            </button>
          {/if}
        {:else}
          <!-- Per-row settings inside the profile dropdown. Unlike the account
             trash (which stops its own mousedown so it does not commit the row),
             this deliberately lets the mousedown bubble to the option row's
             commit — making that profile active AND closing the dropdown. The
             Settings tab always shows the active profile, so the page's request
             names this id and waits for that activation instead of opening on
             the previous profile. -->
          {#snippet instanceTrailing(opt: SelectOption)}
            {@const inst = instances.find((x) => x.id === opt.value)}
            {#if inst}
              {@const settingsLabel = $t('sidebar.instanceSettingsLabel', { name: inst.name })}
              <span class="flex flex-shrink-0 items-center gap-1">
                {#if isRunning(inst.id)}
                  <!-- Inline Stop for a running row. Hidden at rest, revealed on
                       row hover AND keyboard-arrow focus (the Select marks the
                       active row `.is-active`) — the literal Tailwind variants must
                       live in this file for the JIT scanner. tabindex=-1 keeps the
                       Select's aria-activedescendant focus model intact (focus
                       stays on the trigger, like the settings / trash row actions).
                       Its mousedown is stopped so it does NOT commit / switch the
                       row — clicking Stop must not select the instance. Fully
                       keyboard-operable stop for ANY running instance already
                       exists on the aggregate running-instances pill. -->
                  <button
                    type="button"
                    tabindex="-1"
                    class="btn-icon btn-icon-sm btn-icon-danger opacity-0 group-hover:opacity-100 group-[.is-active]:opacity-100"
                    data-testid="sidebar-stop-instance-{inst.id}"
                    aria-label={$t('sidebar.stop')}
                    use:tooltip={{ text: $t('sidebar.stop'), describe: false }}
                    onmousedown={(e) => {
                      e.stopPropagation();
                      e.preventDefault();
                    }}
                    onclick={() => onStopInstance(inst.id)}
                  >
                    <Icon name="stop" size={14} />
                  </button>
                {/if}
                <button
                  type="button"
                  tabindex="-1"
                  class="btn-icon btn-icon-sm"
                  data-testid="sidebar-instance-settings-{inst.id}"
                  aria-label={settingsLabel}
                  use:tooltip={{ text: settingsLabel, describe: false }}
                  onmousedown={(e) => {
                    // The primary button only: a right-click opens the row menu
                    // without picking the row, and a request with no activation
                    // behind it would wait, armed, for the next pick.
                    if (e.button === 0) onOpenInstanceSettings?.(inst.id);
                  }}
                >
                  <Icon name="sliders" size={14} />
                </button>
              </span>
            {/if}
          {/snippet}
          {#snippet instanceLeading(opt: SelectOption)}
            {@const inst = instances.find((x) => x.id === opt.value)}
            {#if inst}
              <!-- One labelled glyph for an exceptional state, the list's only
                   per-instance status since the Manage list went: integrity
                   problems outrank a missing download; a ready, healthy
                   instance shows nothing. Also the trigger's leading snippet,
                   so the active instance carries it while the list is shut. -->
              <!-- Each glyph says what it means on hover, as the running dot does. -->
              <!-- Damage only: Forge files built differently, or not checked, are notes the Integrity
                   section explains, not an exceptional state (spec 2026-10-18). -->
              {#if inst.integrity && summarizeIntegrity(inst.integrity).state === 'damaged'}
                {@const problems = $t('instance.integrity.statusProblems', {
                  count: inst.integrity.problem_count,
                })}
                <span
                  class="order-last inline-flex flex-shrink-0"
                  use:tooltip={{ text: problems, describe: false }}
                >
                  <Icon name="warning" size={14} class="text-warning-text" label={problems} />
                </span>
              {:else if !inst.ready}
                {@const needed = $t('instance.manage.iconDownloadNeeded')}
                <span
                  class="order-last inline-flex flex-shrink-0"
                  use:tooltip={{ text: needed, describe: false }}
                >
                  <Icon name="download" size={14} class="text-muted" label={needed} />
                </span>
              {/if}
              <span class="relative inline-flex flex-shrink-0">
                <InstanceAvatar instance={inst} size={20} />
                {#if isRunning(inst.id)}
                  <!-- Per-instance "running" badge, overlaid on the avatar corner.
                       Reuses the running vocabulary verbatim: `bg-current` ties the
                       fill to `color`, `text-success` sets the green, and
                       `nav-icon-running` pulses `color` (so the fill pulses too),
                       resting saturated under reduced motion — the same keyframe
                       the ModeSwitcher status icons use. The ring matches the row
                       surface so the dot reads as a badge, not part of the icon.
                       Because this snippet is BOTH optionLeading AND valueLeading,
                       the dot also shows on the closed Select trigger's selected
                       instance — the only always-visible instance icon, incl.
                       compact / mini mode (which shrinks the window but keeps this
                       trigger, so a running selected instance stays visible). -->
                  <span
                    class="absolute -bottom-0.5 -right-0.5 h-2 w-2 rounded-full bg-current text-success nav-icon-running ring-2 ring-surface"
                    role="img"
                    aria-label={$t('sidebar.clientRunning')}
                    use:tooltip={{ text: $t('sidebar.clientRunning'), describe: false }}
                    data-testid="sidebar-instance-running-dot-{inst.id}"
                  ></span>
                {/if}
              </span>
            {/if}
          {/snippet}
          <div data-tour="instance-picker">
            <Select
              class="w-full text-sm"
              value={activeInstance?.id ?? ''}
              options={instanceOptions}
              onChange={(v) => onSelectInstance(String(v))}
              ariaLabel={$t('sidebar.instance')}
              filter={instanceFilter}
              optionLeading={instanceLeading}
              valueLeading={instanceLeading}
              optionTrailing={instanceTrailing}
              onOptionContextMenu={(opt, pos, how) => openInstanceMenu(String(opt.value), pos, how)}
            />
          </div>
          {#if instanceMenu}
            <Menu
              items={instanceMenu.items}
              ariaLabel={instanceMenu.label}
              top={instanceMenu.top}
              left={instanceMenu.left}
              width={MENU_WIDTH}
              openedByKeyboard={instanceMenu.openedByKeyboard}
              onClose={closeInstanceMenu}
            />
          {/if}
          {#if isVisible('mods')}
            <!-- A label never breaks inside itself (plan §5b V2: «Папка / модов»): the
                 button is at least as wide as its label. -->
            <div class="flex flex-wrap gap-1">
              {#if isVisible('mods')}
                <ContextMenu
                  items={hideMenuItems('mods')}
                  ariaLabel={$t('sidebar.contextMenuAria')}
                >
                  <button
                    type="button"
                    class="btn-secondary btn-xs flex-1 flex items-center justify-center gap-1 whitespace-nowrap"
                    onclick={onOpenMods}
                  >
                    <Icon name="folderOpen" size={14} />
                    {$t('sidebar.mods')}
                  </button>
                </ContextMenu>
              {/if}
            </div>
          {/if}

          {#if activeInstance}
            {#if running}
              <button
                type="button"
                data-tour="play-btn"
                class="btn-danger btn-lg flex items-center justify-center gap-1.5"
                onclick={onStop}
              >
                <Icon name="stop" size={16} />
                {$t('sidebar.stop')}
              </button>
            {:else if activeInstance.mc_version === ''}
              <span
                class="inline-flex"
                use:tooltip={{ text: $t('sidebar.pickVersionTitle'), describe: false }}
              >
                <button
                  type="button"
                  data-tour="play-btn"
                  class="btn-success btn-lg w-full flex items-center justify-center gap-1.5"
                  disabled
                >
                  <Icon name="play" size={16} />
                  {$t('sidebar.play')}
                </button>
              </span>
            {:else if installing}
              <button type="button" data-tour="play-btn" class="btn-primary btn-lg" disabled>
                <span class="inline-flex items-center justify-center gap-2">
                  <Spinner size="sm" />
                  {$t('sidebar.working')}
                </span>
              </button>
            {:else if playBlockedReason}
              <!-- Data root unavailable (§7 fallback gating): block both
                 Install and Play with the same explanatory tooltip, whichever
                 would otherwise show. -->
              <span
                class="inline-flex w-full"
                use:tooltip={{ text: playBlockedReason, describe: false }}
              >
                <button
                  type="button"
                  data-tour="play-btn"
                  class="btn-primary btn-lg w-full flex items-center justify-center gap-1.5"
                  disabled
                >
                  <Icon name={activeInstance.ready ? 'play' : 'download'} size={16} />
                  {activeInstance.ready ? $t('sidebar.play') : $t('sidebar.install')}
                </button>
              </span>
            {:else if !activeInstance.ready}
              <!--
            Install is the only available action when an instance is not
            yet ready. Make it loud so it reads as clickable — Play and
            Install never appear at the same time, so they don't compete.
          -->
              <button
                type="button"
                data-tour="play-btn"
                class="btn-primary btn-lg flex items-center justify-center gap-1.5"
                onclick={onInstall}
              >
                <Icon name="download" size={16} />
                {$t('sidebar.install')}
              </button>
            {:else}
              <div class="flex gap-1.5">
                <PlayWithWorlds
                  {worlds}
                  {onPlay}
                  {onQuickPlayWorld}
                  menuEnabled={quickPlayMenuEnabled}
                  label={$t('sidebar.play')}
                  menuLabel={$t('sidebar.playWorlds')}
                />
                {#if isVisible('quick_join')}
                  <ContextMenu
                    items={hideMenuItems('quick_join')}
                    ariaLabel={$t('sidebar.contextMenuAria')}
                  >
                    <button
                      type="button"
                      class="btn-success btn-lg px-3"
                      aria-label={$t('sidebar.servers')}
                      use:tooltip={$t('sidebar.servers')}
                      onclick={onOpenQuickJoin}
                    >
                      <Icon name="globe" size={18} />
                    </button>
                  </ContextMenu>
                {/if}
              </div>
            {/if}
          {/if}
        {/if}
      </SidebarSection>
    {:else}
      <ServerSidebarSection />
    {/if}

    <!--
      Content section (§14 sidebar rhythm): the instance-creating actions —
      New instance, Browse modpacks (install a new pack → a new instance) and
      Import from launcher. Client-only. Section + heading render only when ≥1
      member is visible, so hidden buttons leave no orphaned heading.
    -->
    {#if serversUi.mode === 'client' && (isVisible('new_instance') || isVisible('browse_modpacks') || isVisible('import_launcher'))}
      <SidebarSection
        heading={$t('sidebar.sectionContent')}
        headingTestid="sidebar-section-content"
      >
        <HideableSidebarButton
          id="new_instance"
          hideItems={hideMenuItems('new_instance')}
          contextMenuAria={$t('sidebar.contextMenuAria')}
          testid="sidebar-new-instance"
          disabledTooltip={createBlockedReason}
          onclick={onCreateInstance}
        >
          <Icon name="plus" size={14} />
          {$t('sidebar.newInstance')}
        </HideableSidebarButton>
        <HideableSidebarButton
          id="browse_modpacks"
          hideItems={hideMenuItems('browse_modpacks')}
          contextMenuAria={$t('sidebar.contextMenuAria')}
          testid="sidebar-open-modpacks"
          dataTour="open-modpacks"
          onclick={onOpenModpacks}
        >
          <span class="relative inline-flex items-center gap-1">
            <Icon name="package" size={14} class={rainbowFx.enabled ? 'icon-rainbow-hover' : ''} />
            {$t('sidebar.browseModpacks')}
            {#if modpackUpdates.updateCount > 0}
              <CountPill
                size="md"
                class="ml-1"
                title={$t('sidebar.modpackUpdatesBadge', {
                  count: modpackUpdates.updateCount,
                })}
                testid="sidebar-modpack-updates-badge"
              >
                {modpackUpdates.updateCount}
              </CountPill>
            {/if}
          </span>
        </HideableSidebarButton>
        <HideableSidebarButton
          id="import_launcher"
          hideItems={hideMenuItems('import_launcher')}
          contextMenuAria={$t('sidebar.contextMenuAria')}
          testid="sidebar-open-launcher-import"
          disabledTooltip={launcherImportBlockedReason}
          onclick={onOpenLauncherImport}
        >
          <Icon name="download" size={14} />
          {$t('sidebar.importLauncher')}
        </HideableSidebarButton>
      </SidebarSection>
    {/if}

    <!--
      View section: look at what your play produced — Gallery (your
      screenshots) and Logs (the active instance's game logs). Both are
      client concepts. Section + heading render only when ≥1 member is
      visible.
    -->
    {#if serversUi.mode === 'client' && (isVisible('gallery') || isVisible('logs'))}
      <SidebarSection heading={$t('sidebar.sectionView')} headingTestid="sidebar-section-view">
        <HideableSidebarButton
          id="gallery"
          hideItems={hideMenuItems('gallery')}
          contextMenuAria={$t('sidebar.contextMenuAria')}
          testid="sidebar-open-gallery"
          onclick={onOpenGallery}
        >
          <Icon name="gallery" size={14} />
          {$t('sidebar.gallery')}
        </HideableSidebarButton>
        <HideableSidebarButton
          id="logs"
          hideItems={hideMenuItems('logs')}
          contextMenuAria={$t('sidebar.contextMenuAria')}
          testid="sidebar-open-logs"
          onclick={onOpenLogs}
        >
          <NavStatusIcon
            name="scrollText"
            size={14}
            iconClass={logsVisual.iconClass}
            statusLabel={logsStatusLabel}
          />
          {$t('sidebar.logs')}
          {#if logsVisual.wrench}
            <NavFixWrench label={$t('sidebar.logsFixAvailable')} testid="logs-button-fix-badge" />
          {/if}
        </HideableSidebarButton>
      </SidebarSection>
    {/if}

    <!--
      Settings footer: app-level config. The only heading-less lower section —
      that is what reads it as a footer (uniform divider, no mt-auto; empty
      space still pools below it). Not hideable, so it always renders,
      including in servers mode where Content/View are hidden as client
      concepts.
    -->
    <SidebarSection>
      <button
        type="button"
        class="btn-secondary btn-xs w-full flex items-center justify-center gap-1"
        onclick={() => (settingsOpen.value = { tab: 'appearance' })}
      >
        <Icon name="settings" size={14} />
        {$t('settings.title')}
      </button>
    </SidebarSection>
  </div>
</aside>

{#if hideCandidate}
  <HideButtonConfirmDialog
    label={hideCandidateLabel()}
    onCancel={() => (hideCandidate = null)}
    onConfirm={() => {
      if (hideCandidate) void setHidden(hideCandidate, true);
      hideCandidate = null;
    }}
  />
{/if}

{#if accountRequiredOpen}
  <AccountRequiredDialog onClose={() => (accountRequiredOpen = false)} />
{/if}

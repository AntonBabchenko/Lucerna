<script lang="ts">
  import type { Account, InstanceWithStatus, WorldQuickEntry } from '$lib/ipc/bindings';
  import PlayWithWorlds from '$lib/layout/PlayWithWorlds.svelte';
  import InstanceAvatar from '$lib/instances/InstanceAvatar.svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';
  import { diagnosisStatus } from '$lib/logs/log-diagnosis.svelte';
  import InstanceConceptTooltip from '$lib/onboarding/InstanceConceptTooltip.svelte';
  import { settingsOpen } from '$lib/settings/state.svelte';
  import MicrosoftSignInButton from '$lib/accounts/MicrosoftSignInButton.svelte';
  import PlayerHead from '$lib/accounts/PlayerHead.svelte';
  import Select from '$lib/ui/Select.svelte';
  import type { SelectOption } from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
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
  import Menu from '$lib/ui/Menu.svelte';
  import HideButtonConfirmDialog from '$lib/layout/HideButtonConfirmDialog.svelte';
  import AccountRequiredDialog from '$lib/layout/AccountRequiredDialog.svelte';
  import { SIDEBAR_BUTTONS, type SidebarButtonId } from '$lib/layout/sidebar-buttons';
  import SidebarSection from '$lib/layout/SidebarSection.svelte';
  import HideableSidebarButton from '$lib/layout/HideableSidebarButton.svelte';

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
    onOpenManage,
    onManageInstance = () => {},
    onCloneInstance = () => {},
    onCreateShortcut,
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
    onOpenManage: () => void;
    // Manage a specific profile (the per-row icon in the profile dropdown):
    // seeds the manage modal's detail selection to this id, independent of the
    // async active-instance switch. Defaults to a no-op.
    onManageInstance?: (instanceId: string) => void;
    // Open the clone dialog for a specific profile (the right-click menu on a
    // row in the profile dropdown). Defaults to a no-op.
    onCloneInstance?: (instanceId: string) => void;
    /** Undefined on platforms without desktop-shortcut support — the menu item
     *  is then omitted rather than shown disabled. */
    onCreateShortcut?: (instanceId: string) => void;
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
    // gating): disables Play/Install (with an explanatory tooltip) and the
    // empty-state "Create instance" shortcut. See data-root-gating.ts.
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
  const instanceOptions = $derived(
    instances.map((i) => ({
      value: i.id,
      icon:
        i.integrity && !i.integrity.healthy
          ? ('warning' as const)
          : modpackUpdates.hasUpdate(i.id)
            ? ('update' as const)
            : i.ready
              ? ('success' as const)
              : ('download' as const),
      label: `${i.name} · ${displayLoader(i.loader)} ${i.mc_version || $t('sidebar.pickMcVersion')}`,
    })),
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
  let instanceMenu = $state<{ id: string; top: number; left: number } | null>(null);
  // Plain (non-reactive) snapshot of the menu's target id: Menu calls
  // onClose() — which nulls `instanceMenu` — BEFORE onSelect(), so the item
  // handler must not read the reactive state (a template `{@const}` re-derives
  // at call time and crashes on null).
  let instanceMenuTargetId = '';
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
          <p class="text-xs text-muted">{$t('sidebar.noAccounts')}</p>
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
        {:else if instances.length === 0}
          <p class="text-xs text-muted">{$t('sidebar.noInstances')}</p>
          {#if createBlockedReason}
            <span class="inline-flex" use:tooltip={{ text: createBlockedReason, describe: false }}>
              <button type="button" class="btn-primary btn-xs" disabled>
                {$t('sidebar.createInstance')}
              </button>
            </span>
          {:else}
            <button type="button" class="btn-primary btn-xs" onclick={onOpenManage}>
              {$t('sidebar.createInstance')}
            </button>
          {/if}
        {:else}
          <!-- Per-row Manage inside the profile dropdown. Unlike the account
             trash (which stops its own mousedown so it does not commit the row),
             this deliberately lets the mousedown bubble to the option row's
             commit — selecting that profile (making it active) AND closing the
             dropdown. It opens Manage via onManageInstance(id) with the clicked
             id, so the modal seeds its detail to THIS profile directly rather
             than racing the async active-instance switch. -->
          {#snippet instanceTrailing(opt: SelectOption)}
            {@const inst = instances.find((x) => x.id === opt.value)}
            {#if inst}
              {@const manageLabel = $t('sidebar.manageInstanceLabel', { name: inst.name })}
              <span class="flex flex-shrink-0 items-center gap-1">
                {#if isRunning(inst.id)}
                  <!-- Inline Stop for a running row. Hidden at rest, revealed on
                       row hover AND keyboard-arrow focus (the Select marks the
                       active row `.is-active`) — the literal Tailwind variants must
                       live in this file for the JIT scanner. tabindex=-1 keeps the
                       Select's aria-activedescendant focus model intact (focus
                       stays on the trigger, like the Manage / trash row actions).
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
                  data-testid="sidebar-manage-instance-{inst.id}"
                  aria-label={manageLabel}
                  use:tooltip={{ text: manageLabel, describe: false }}
                  onmousedown={() => onManageInstance(inst.id)}
                >
                  <Icon name="sliders" size={14} />
                </button>
              </span>
            {/if}
          {/snippet}
          {#snippet instanceLeading(opt: SelectOption)}
            {@const inst = instances.find((x) => x.id === opt.value)}
            {#if inst}
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
              optionLeading={instanceLeading}
              valueLeading={instanceLeading}
              optionTrailing={instanceTrailing}
              onOptionContextMenu={(opt, pos) => {
                instanceMenuTargetId = String(opt.value);
                instanceMenu = { id: instanceMenuTargetId, top: pos.y, left: pos.x };
              }}
            />
          </div>
          {#if instanceMenu}
            <Menu
              items={[
                {
                  label: $t('sidebar.cloneInstance'),
                  icon: 'copy',
                  testId: 'sidebar-ctx-clone-instance',
                  onSelect: () => onCloneInstance(instanceMenuTargetId),
                },
                // Omitted entirely where the OS has no shortcut support (the page
                // leaves the handler undefined) rather than shown disabled.
                ...(onCreateShortcut
                  ? [
                      {
                        label: $t('sidebar.createShortcut'),
                        icon: 'monitor' as const,
                        testId: 'sidebar-ctx-create-shortcut',
                        onSelect: () => onCreateShortcut?.(instanceMenuTargetId),
                      },
                    ]
                  : []),
              ]}
              ariaLabel={$t('sidebar.instanceMenuAria')}
              top={instanceMenu.top}
              left={instanceMenu.left}
              width={220}
              onClose={() => (instanceMenu = null)}
            />
          {/if}
          {#if isVisible('manage') || isVisible('mods')}
            <div class="flex gap-1">
              {#if isVisible('manage')}
                <ContextMenu
                  items={hideMenuItems('manage')}
                  ariaLabel={$t('sidebar.contextMenuAria')}
                >
                  <button
                    type="button"
                    data-tour="manage-btn"
                    class="btn-secondary btn-xs flex-1 flex items-center justify-center gap-1"
                    onclick={onOpenManage}
                  >
                    <Icon name="sliders" size={14} />
                    {$t('sidebar.manage')}
                  </button>
                </ContextMenu>
              {/if}
              {#if isVisible('mods')}
                <ContextMenu
                  items={hideMenuItems('mods')}
                  ariaLabel={$t('sidebar.contextMenuAria')}
                >
                  <button
                    type="button"
                    class="btn-secondary btn-xs flex-1 flex items-center justify-center gap-1"
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
      Browse modpacks (install a new pack → a new instance) and Import from
      launcher. Client-only. Section + heading render only when ≥1 member is
      visible, so a hidden pair leaves no orphaned heading.
    -->
    {#if serversUi.mode === 'client' && (isVisible('browse_modpacks') || isVisible('import_launcher'))}
      <SidebarSection
        heading={$t('sidebar.sectionContent')}
        headingTestid="sidebar-section-content"
      >
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
              <span
                class="ml-1 inline-flex min-w-[18px] h-[18px] items-center justify-center rounded-full bg-success px-1 text-[10px] font-semibold text-white"
                use:tooltip={$t('sidebar.modpackUpdatesBadge', {
                  count: modpackUpdates.updateCount,
                })}
                data-testid="sidebar-modpack-updates-badge"
              >
                {modpackUpdates.updateCount}
              </span>
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

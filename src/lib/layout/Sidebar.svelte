<script lang="ts">
  import type { Account, InstanceWithStatus, WorldQuickEntry } from '$lib/ipc/bindings';
  import PlayWithWorlds from '$lib/layout/PlayWithWorlds.svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';
  import { hasDiagnosisIndicator } from '$lib/logs/log-diagnosis.svelte';
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

  let {
    accounts,
    activeAccount,
    instances,
    activeInstance,
    onSelectAccount,
    onRemoveAccount,
    onAddOffline,
    onSelectInstance,
    onOpenManage,
    onOpenMods,
    onOpenLogs,
    onOpenModpacks,
    onOpenLauncherImport,
    running,
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
    onOpenServers = () => {},
  }: {
    accounts: Account[];
    activeAccount: Account | null;
    instances: InstanceWithStatus[];
    activeInstance: InstanceWithStatus | null;
    onSelectAccount: (id: string) => void;
    onRemoveAccount: (id: string) => void;
    onAddOffline: (name: string) => void;
    onSelectInstance: (id: string) => void;
    onOpenManage: () => void;
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
    onOpenServers?: () => void;
  } = $props();

  let showAddOfflineInput = $state(false);
  let offlineNameDraft = $state('');

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
</script>

<aside data-sidebar class="h-full bg-base border-r border-border-subtle p-3 overflow-y-auto">
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

    <div class="flex flex-col gap-1 pt-3 border-t border-border-subtle" data-tour="account-section">
      <div class="text-xs uppercase tracking-wide text-muted">{$t('sidebar.account')}</div>
      {#if accounts.length === 0}
        <p class="text-xs text-muted">{$t('sidebar.noAccounts')}</p>
      {:else}
        {#snippet accountLeading(opt: SelectOption)}
          {@const acc = accounts.find((a) => a.id === opt.value)}
          {#if acc}
            <PlayerHead uuid={acc.uuid} name={acc.name} size={20} />
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
      <button
        type="button"
        class="btn-secondary btn-xs w-full flex items-center justify-center gap-1"
        onclick={() => (showAddOfflineInput = !showAddOfflineInput)}
      >
        <Icon name="userPlus" size={14} />
        {$t('sidebar.addOffline')}
      </button>
      {#if showAddOfflineInput}
        <div class="flex flex-col gap-1 mt-1">
          <input
            class="border rounded px-2 py-1 text-sm"
            placeholder={$t('sidebar.playerNamePlaceholder')}
            maxlength="16"
            bind:value={offlineNameDraft}
          />
          <div class="flex gap-1">
            <button
              type="button"
              class="btn-primary btn-xs flex-1"
              onclick={() => {
                onAddOffline(offlineNameDraft.trim());
                showAddOfflineInput = false;
                offlineNameDraft = '';
              }}
            >
              {$t('sidebar.addAccountConfirm')}
            </button>
            <button
              type="button"
              class="btn-secondary btn-xs flex-1"
              onclick={() => {
                showAddOfflineInput = false;
                offlineNameDraft = '';
              }}
            >
              {$t('common.cancel')}
            </button>
          </div>
        </div>
      {/if}
      <div class="mt-2">
        <MicrosoftSignInButton
          bind:signingIn={msSigningIn}
          onSignedIn={(account) => onMicrosoftSignedIn?.(account)}
          onError={(err) => onMicrosoftError?.(err)}
        />
      </div>
    </div>

    <div class="flex flex-col gap-1 pt-3 border-t border-border-subtle">
      <div class="text-xs uppercase tracking-wide text-muted flex items-center gap-1">
        <span>{$t('sidebar.instance')}</span>
        <InstanceConceptTooltip />
      </div>
      {#if instances.length === 0}
        <p class="text-xs text-muted">{$t('sidebar.noInstances')}</p>
        <button type="button" class="btn-primary btn-xs" onclick={onOpenManage}>
          {$t('sidebar.createInstance')}
        </button>
      {:else}
        <div data-tour="instance-picker">
          <Select
            class="w-full text-sm"
            value={activeInstance?.id ?? ''}
            options={instanceOptions}
            onChange={(v) => onSelectInstance(String(v))}
            ariaLabel={$t('sidebar.instance')}
          />
        </div>
        <div class="flex gap-1">
          <button
            type="button"
            data-tour="manage-btn"
            class="btn-secondary btn-xs flex-1 flex items-center justify-center gap-1"
            onclick={onOpenManage}
          >
            <Icon name="sliders" size={14} />
            {$t('sidebar.manage')}
          </button>
          <button
            type="button"
            class="btn-secondary btn-xs flex-1 flex items-center justify-center gap-1"
            onclick={onOpenMods}
          >
            <Icon name="folderOpen" size={14} />
            {$t('sidebar.mods')}
          </button>
        </div>

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
              <button
                type="button"
                class="btn-success btn-lg px-3"
                aria-label={$t('sidebar.servers')}
                use:tooltip={$t('sidebar.servers')}
                onclick={onOpenQuickJoin}
              >
                <Icon name="globe" size={18} />
              </button>
            </div>
          {/if}
        {/if}
      {/if}
    </div>

    <!--
    This action group clusters directly under the Install/Play block (no
    mt-auto) so the controls read as one unit; empty space pools at the
    bottom of the sidebar rather than between the button and these actions.
  -->
    <div class="flex flex-col gap-3 pt-3 border-t border-border-subtle">
      <!--
      Modpacks live at the sidebar level (not the per-instance tab strip)
      because installing a pack creates a NEW instance, so there's nothing
      "current instance" about the action.
    -->
      <button
        type="button"
        class="btn-secondary btn-sm flex items-center justify-center gap-1.5"
        data-tour="open-modpacks"
        data-testid="sidebar-open-modpacks"
        onclick={onOpenModpacks}
      >
        <span class="relative inline-flex items-center gap-1.5">
          <Icon name="package" size={16} class={rainbowFx.enabled ? 'icon-rainbow-hover' : ''} />
          {$t('sidebar.browseModpacks')}
          {#if modpackUpdates.updateCount > 0}
            <span
              class="ml-1 inline-flex min-w-[18px] h-[18px] items-center justify-center rounded-full bg-success px-1 text-[10px] font-semibold text-white"
              use:tooltip={$t('sidebar.modpackUpdatesBadge', { count: modpackUpdates.updateCount })}
              data-testid="sidebar-modpack-updates-badge"
            >
              {modpackUpdates.updateCount}
            </span>
          {/if}
        </span>
      </button>
      <button
        type="button"
        class="btn-secondary btn-sm flex items-center justify-center gap-1.5"
        data-testid="sidebar-open-launcher-import"
        onclick={onOpenLauncherImport}
      >
        <Icon name="download" size={16} />
        {$t('sidebar.importLauncher')}
      </button>
      <button
        type="button"
        class="btn-secondary btn-sm flex items-center justify-center gap-1.5"
        data-testid="sidebar-open-servers"
        onclick={onOpenServers}
      >
        <Icon name="server" size={16} />
        {$t('sidebar.servers')}
      </button>
      <div class="flex gap-1">
        <button
          type="button"
          class="btn-secondary btn-xs flex-1 flex items-center justify-center gap-1 relative"
          data-testid="sidebar-open-logs"
          onclick={onOpenLogs}
        >
          <Icon name="scrollText" size={14} />
          {$t('sidebar.logs')}
          {#if hasDiagnosisIndicator()}
            <span
              class="absolute -right-0.5 -top-0.5 h-2 w-2 rounded-full bg-warning-text"
              data-testid="logs-button-badge"
              aria-hidden="true"
            ></span>
          {/if}
        </button>
        <button
          type="button"
          class="btn-secondary btn-xs flex-1 flex items-center justify-center gap-1"
          onclick={() => (settingsOpen.value = { tab: 'appearance' })}
        >
          <Icon name="settings" size={14} />
          {$t('settings.title')}
        </button>
      </div>
    </div>
  </div>
</aside>

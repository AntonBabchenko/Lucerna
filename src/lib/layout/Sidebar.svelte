<script lang="ts">
  import type { Account, InstanceWithStatus } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';
  import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';
  import InstanceConceptTooltip from '$lib/onboarding/InstanceConceptTooltip.svelte';
  import { settingsOpen } from '$lib/settings/state.svelte';
  import MicrosoftSignInButton from '$lib/accounts/MicrosoftSignInButton.svelte';
  import PlayerHead from '$lib/accounts/PlayerHead.svelte';
  import Select from '$lib/ui/Select.svelte';
  import type { SelectOption } from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { Icon } from '$lib/ui/icons';
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
    msSigningIn = $bindable(false),
    onMicrosoftSignedIn,
    onMicrosoftError,
    compact = false,
    onToggleCompact = () => {},
    quickPlaySupported = false,
    onOpenQuickJoin = () => {},
  }: {
    accounts: Account[];
    activeAccount: Account | null;
    instances: InstanceWithStatus[];
    activeInstance: InstanceWithStatus | null;
    onSelectAccount: (id: string) => void;
    onRemoveAccount: () => void;
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
    msSigningIn?: boolean;
    onMicrosoftSignedIn?: (account: unknown) => void;
    onMicrosoftError?: (err: unknown) => void;
    compact?: boolean;
    onToggleCompact?: () => void;
    quickPlaySupported?: boolean;
    onOpenQuickJoin?: () => void;
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

<aside
  data-sidebar
  class="h-full bg-base border-r border-border-subtle p-3 flex flex-col gap-3 overflow-y-auto"
>
  <div class="flex items-center justify-between">
    <span class="font-bold text-lg text-primary">Lucerna</span>
    <button
      type="button"
      class="flex h-7 w-7 items-center justify-center rounded-md border border-border bg-surface text-muted hover:border-accent hover:text-accent"
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
      <Select
        class="w-full text-sm"
        value={activeAccount?.id ?? ''}
        options={accountOptions}
        onChange={(v) => onSelectAccount(String(v))}
        ariaLabel={$t('sidebar.account')}
        optionLeading={accountLeading}
        valueLeading={accountLeading}
      />
    {/if}
    <div class="flex gap-1">
      <button
        type="button"
        class="btn-secondary btn-xs flex-1"
        onclick={() => (showAddOfflineInput = !showAddOfflineInput)}
      >
        {$t('sidebar.addOffline')}
      </button>
      <span
        class="inline-flex flex-1"
        use:tooltip={{
          text: !activeAccount ? $t('sidebar.removeAccountDisabled') : '',
          describe: false,
        }}
      >
        <button
          type="button"
          class="btn-secondary btn-xs w-full"
          disabled={!activeAccount}
          onclick={onRemoveAccount}
        >
          {$t('sidebar.removeAccount')}
        </button>
      </span>
    </div>
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
          <Icon name="settings" size={14} />
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
          <button type="button" data-tour="play-btn" class="btn-danger btn-lg" onclick={onStop}>
            {$t('sidebar.stop')}
          </button>
        {:else if activeInstance.mc_version === ''}
          <span
            class="inline-flex"
            use:tooltip={{ text: $t('sidebar.pickVersionTitle'), describe: false }}
          >
            <button type="button" data-tour="play-btn" class="btn-success btn-lg w-full" disabled>
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
            <button
              type="button"
              data-tour="play-btn"
              class="btn-success btn-lg flex-1"
              onclick={onPlay}
            >
              {$t('sidebar.play')}
            </button>
            {#if quickPlaySupported}
              <button
                type="button"
                class="btn-success btn-lg px-3"
                aria-label={$t('sidebar.joinServer')}
                use:tooltip={$t('sidebar.joinServer')}
                onclick={onOpenQuickJoin}
              >
                <Icon name="globe" size={18} />
              </button>
            {/if}
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
      class="btn-secondary btn-sm flex items-center justify-center gap-1.5 hover:bg-accent-soft hover:border-accent"
      data-tour="open-modpacks"
      data-testid="sidebar-open-modpacks"
      onclick={onOpenModpacks}
    >
      <span class="relative inline-flex items-center gap-1.5">
        <Icon name="package" size={16} />
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
      class="btn-secondary btn-sm flex items-center justify-center gap-1.5 hover:bg-accent-soft hover:border-accent"
      data-testid="sidebar-open-launcher-import"
      onclick={onOpenLauncherImport}
    >
      <Icon name="download" size={16} />
      {$t('sidebar.importLauncher')}
    </button>
    <div class="flex gap-1">
      <button
        type="button"
        class="btn-secondary btn-xs flex-1 flex items-center justify-center gap-1"
        onclick={onOpenLogs}
      >
        <Icon name="scrollText" size={14} />
        {$t('sidebar.logs')}
      </button>
      <button
        type="button"
        class="btn-secondary btn-xs flex-1"
        onclick={() => (settingsOpen.value = { tab: 'general' })}
      >
        {$t('settings.title')}
      </button>
    </div>
  </div>
</aside>

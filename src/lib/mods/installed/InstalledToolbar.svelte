<script lang="ts">
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import Select from '$lib/ui/Select.svelte';
  import ToggleChipGroup from '$lib/ui/ToggleChipGroup.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import type { SortBy, ViewFilter } from './installed-filters.svelte';

  let {
    counts,
    filter = $bindable(),
    sortBy = $bindable(),
    viewFilter = $bindable(),
    busy,
    checking,
    graphLoading,
    updateCount,
    onCheckUpdates,
    onRecheckDeps,
    onUpdateAll,
    checkingCompat,
    onCheckCompat,
  }: {
    counts: {
      total: number;
      enabled: number;
      disabled: number;
      updates: number;
      issues: number;
      incompatible: number;
    };
    filter: string;
    sortBy: SortBy;
    viewFilter: ViewFilter;
    busy: boolean;
    checking: boolean;
    graphLoading: boolean;
    updateCount: number;
    onCheckUpdates: () => void;
    onRecheckDeps: () => void;
    onUpdateAll: () => void;
    checkingCompat: boolean;
    onCheckCompat: () => void;
  } = $props();

  const checkDisabledReason = $derived(
    counts.total === 0
      ? $t('mods.installed.disabledNoMods')
      : busy
        ? $t('mods.installed.disabledBusy')
        : '',
  );

  const sortOptions = $derived([
    { value: 'name-asc', label: $t('mods.installed.sortNameAsc') },
    { value: 'name-desc', label: $t('mods.installed.sortNameDesc') },
    { value: 'recent', label: $t('mods.installed.sortRecent') },
    { value: 'source', label: $t('mods.installed.sortSource') },
  ]);

  // One mutually-exclusive filter group. All / Enabled / Disabled are always
  // present; Updates / Issues / Incompatible appear only when there is something
  // to show. Each option carries its own tone so the chips read as distinct
  // kinds (state vs status) while behaving as a single pick-one set. The count
  // rides the chip's count badge; the label stays the plain word so the
  // accessible name matches the simple /All/ etc. patterns the tests use.
  const filterOptions = $derived([
    {
      value: 'all',
      label: $t('mods.installed.filterAllLabel'),
      tone: 'neutral' as const,
      count: counts.total,
      testId: 'installed-filter-all',
    },
    {
      value: 'enabled',
      label: $t('mods.installed.filterEnabledLabel'),
      tone: 'success' as const,
      count: counts.enabled,
      testId: 'installed-filter-enabled',
    },
    {
      value: 'disabled',
      label: $t('mods.installed.filterDisabledLabel'),
      tone: 'muted' as const,
      count: counts.disabled,
      testId: 'installed-filter-disabled',
    },
    ...(counts.updates > 0
      ? [
          {
            value: 'updates',
            label: $t('mods.installed.filterUpdatesLabel'),
            tone: 'warning' as const,
            icon: 'arrowUp' as const,
            count: counts.updates,
            testId: 'installed-filter-updates',
          },
        ]
      : []),
    ...(counts.issues > 0
      ? [
          {
            value: 'issues',
            label: $t('mods.installed.filterIssuesLabel'),
            tone: 'danger' as const,
            icon: 'warning' as const,
            count: counts.issues,
            testId: 'installed-filter-issues',
          },
        ]
      : []),
    ...(counts.incompatible > 0
      ? [
          {
            value: 'incompatible',
            label: $t('mods.installed.filterIncompatibleLabel'),
            tone: 'danger' as const,
            icon: 'warning' as const,
            count: counts.incompatible,
            testId: 'installed-filter-incompatible',
          },
        ]
      : []),
  ]);
</script>

<div class="mb-2 space-y-2">
  {#if counts.total > 0}
    <div class="text-xs text-muted flex gap-3">
      <span
        >{$t('mods.installed.statsTotal')}
        <span class="font-medium text-secondary">{counts.total}</span></span
      >
      <span
        >{$t('mods.installed.statsEnabled')}
        <span class="font-medium text-success">{counts.enabled}</span></span
      >
      <span
        >{$t('mods.installed.statsDisabled')}
        <span class="font-medium text-secondary">{counts.disabled}</span></span
      >
    </div>
  {/if}
  <div class="flex flex-wrap gap-2 items-center">
    <input
      type="search"
      placeholder={$t('mods.installed.filterPlaceholder')}
      aria-label={$t('mods.installed.filterAriaLabel')}
      class="flex-1 border border-border-emphasis rounded px-3 py-1.5 text-sm"
      bind:value={filter}
    />
    <div class="text-xs text-secondary inline-flex items-center gap-1">
      {$t('mods.installed.sortLabel')}
      <Select
        class="text-xs"
        ariaLabel={$t('mods.installed.sortLabel')}
        value={sortBy}
        options={sortOptions}
        onChange={(v) => (sortBy = String(v) as SortBy)}
      />
    </div>
    <span class="inline-flex" use:tooltip={{ text: checkDisabledReason, describe: false }}>
      <BusyButton
        busy={checking}
        disabled={busy || counts.total === 0}
        class="btn-secondary btn-xs"
        onclick={onCheckUpdates}
      >
        <Icon name="refresh" class="icon-spin-hover" />
        {checking ? $t('mods.card.checking') : $t('mods.installed.checkUpdates')}
      </BusyButton>
    </span>
    <span class="inline-flex" use:tooltip={{ text: checkDisabledReason, describe: false }}>
      <BusyButton
        busy={checkingCompat}
        disabled={busy || counts.total === 0}
        class="btn-secondary btn-xs"
        onclick={onCheckCompat}
      >
        {checkingCompat ? $t('mods.installed.checkingCompat') : $t('mods.installed.checkCompat')}
      </BusyButton>
    </span>
    <button
      type="button"
      class="btn-secondary btn-xs inline-flex items-center gap-1.5"
      disabled={graphLoading}
      onclick={onRecheckDeps}
    >
      {#if graphLoading}
        <Spinner size="sm" labelPlacement="right" label={$t('mods.installed.resolvingDeps')} />
      {:else}
        <Icon name="refresh" class="icon-spin-hover" />
        {$t('mods.installed.recheckDeps')}
      {/if}
    </button>
    {#if updateCount > 0}
      <BusyButton {busy} class="btn-warning btn-xs" onclick={onUpdateAll}>
        {$t('mods.installed.updateAll', { count: updateCount })}
      </BusyButton>
    {/if}
  </div>
  {#if counts.total > 0}
    <ToggleChipGroup
      options={filterOptions}
      value={viewFilter}
      onChange={(v) => (viewFilter = v as ViewFilter)}
      ariaLabel={$t('mods.installed.filterGroupAriaLabel')}
    />
  {/if}
</div>

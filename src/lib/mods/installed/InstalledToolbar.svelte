<script lang="ts">
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Select from '$lib/ui/Select.svelte';
  import { Icon } from '$lib/ui/icons';
  import DensityToggle from '$lib/mods/DensityToggle.svelte';
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
  // present; Updates / Issues appear only when there is something to show. Each
  // option carries its own active-state colour so the chips read as distinct
  // kinds (state vs status) while behaving as a single pick-one set.
  const filterOptions = $derived([
    {
      value: 'all' as const,
      label: $t('mods.installed.filterAll', { count: counts.total }),
      activeClass: 'bg-accent-soft text-accent font-medium',
    },
    {
      value: 'enabled' as const,
      label: $t('mods.installed.filterEnabled', { count: counts.enabled }),
      activeClass: 'bg-success-bg text-success font-medium',
    },
    {
      value: 'disabled' as const,
      label: $t('mods.installed.filterDisabled', { count: counts.disabled }),
      activeClass: 'bg-subtle text-secondary font-medium',
    },
    ...(counts.updates > 0
      ? [
          {
            value: 'updates' as const,
            label: $t('mods.installed.filterUpdates', { count: counts.updates }),
            activeClass: 'bg-warning-bg text-warning-text font-medium',
          },
        ]
      : []),
    ...(counts.issues > 0
      ? [
          {
            value: 'issues' as const,
            label: $t('mods.installed.filterIssues', { count: counts.issues }),
            activeClass: 'bg-danger-bg text-danger font-medium',
          },
        ]
      : []),
    ...(counts.incompatible > 0
      ? [
          {
            value: 'incompatible' as const,
            label: $t('mods.installed.filterIncompatible', { count: counts.incompatible }),
            activeClass: 'bg-warning-bg text-warning-text font-medium',
          },
        ]
      : []),
  ]);

  // WCAG radiogroup keyboard pattern. Arrow / Home / End moves selection within
  // the group; the newly-checked radio gets focus. Roving tabindex (0 on
  // checked, -1 elsewhere) keeps the whole group as one tab stop.
  function handleFilterKey(e: KeyboardEvent) {
    const values = filterOptions.map((o) => o.value);
    const i = values.indexOf(viewFilter as (typeof values)[number]);
    const len = values.length;
    let next: ViewFilter | null = null;
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = values[(i + 1) % len];
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = values[(i - 1 + len) % len];
    else if (e.key === 'Home') next = values[0];
    else if (e.key === 'End') next = values[len - 1];
    if (next !== null) {
      e.preventDefault();
      viewFilter = next;
      const target = e.currentTarget as HTMLElement | null;
      target?.querySelector<HTMLButtonElement>(`button[data-value="${next}"]`)?.focus();
    }
  }
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
      <Icon name="refresh" class="icon-spin-hover" />
      {graphLoading ? $t('mods.installed.resolvingDeps') : $t('mods.installed.recheckDeps')}
    </button>
    {#if updateCount > 0}
      <BusyButton {busy} class="btn-warning btn-xs" onclick={onUpdateAll}>
        {$t('mods.installed.updateAll', { count: updateCount })}
      </BusyButton>
    {/if}
    <DensityToggle />
  </div>
  {#if counts.total > 0}
    <div
      role="radiogroup"
      aria-label={$t('mods.installed.filterGroupAriaLabel')}
      tabindex={-1}
      class="flex flex-wrap gap-1 text-xs"
      onkeydown={handleFilterKey}
    >
      {#each filterOptions as opt (opt.value)}
        <button
          type="button"
          role="radio"
          aria-checked={viewFilter === opt.value}
          tabindex={viewFilter === opt.value ? 0 : -1}
          data-value={opt.value}
          class={`btn-secondary btn-xs inline-flex items-center gap-1 ${viewFilter === opt.value ? opt.activeClass : ''}`}
          onclick={() => (viewFilter = opt.value)}
        >
          {#if opt.value === 'updates'}
            <Icon name="arrowUp" size={12} />
          {:else if opt.value === 'issues' || opt.value === 'incompatible'}
            <Icon name="warning" size={12} />
          {/if}
          {opt.label}
        </button>
      {/each}
    </div>
  {/if}
</div>

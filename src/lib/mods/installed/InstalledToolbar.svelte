<script lang="ts">
  import { t } from '$lib/i18n';
  import Select from '$lib/ui/Select.svelte';
  import type { EnabledFilter, QuickFilter, SortBy } from './installed-filters.svelte';

  let {
    counts,
    filter = $bindable(),
    sortBy = $bindable(),
    enabledFilter = $bindable(),
    quickFilter = $bindable(),
    busy,
    checking,
    graphLoading,
    updateCount,
    onCheckUpdates,
    onRecheckDeps,
    onUpdateAll,
  }: {
    counts: {
      total: number;
      enabled: number;
      disabled: number;
      updates: number;
      issues: number;
    };
    filter: string;
    sortBy: SortBy;
    enabledFilter: EnabledFilter;
    quickFilter: QuickFilter;
    busy: boolean;
    checking: boolean;
    graphLoading: boolean;
    updateCount: number;
    onCheckUpdates: () => void;
    onRecheckDeps: () => void;
    onUpdateAll: () => void;
  } = $props();

  const sortOptions = $derived([
    { value: 'name-asc', label: $t('mods.installed.sortNameAsc') },
    { value: 'name-desc', label: $t('mods.installed.sortNameDesc') },
    { value: 'recent', label: $t('mods.installed.sortRecent') },
    { value: 'source', label: $t('mods.installed.sortSource') },
  ]);

  // WCAG radiogroup keyboard pattern. Arrow / Home / End moves selection within
  // the group; the newly-checked radio gets focus. Roving tabindex (0 on
  // checked, -1 elsewhere) keeps the whole group as one tab stop.
  const FILTER_VALUES = ['all', 'enabled', 'disabled'] as const;
  function handleFilterKey(e: KeyboardEvent) {
    const i = FILTER_VALUES.indexOf(enabledFilter);
    let next: (typeof FILTER_VALUES)[number] | null = null;
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown')
      next = FILTER_VALUES[(i + 1) % FILTER_VALUES.length];
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp')
      next = FILTER_VALUES[(i - 1 + FILTER_VALUES.length) % FILTER_VALUES.length];
    else if (e.key === 'Home') next = FILTER_VALUES[0];
    else if (e.key === 'End') next = FILTER_VALUES[FILTER_VALUES.length - 1];
    if (next !== null) {
      e.preventDefault();
      enabledFilter = next;
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
    <button
      type="button"
      class="btn-secondary btn-xs"
      disabled={busy || checking || counts.total === 0}
      onclick={onCheckUpdates}
    >
      {checking ? $t('mods.card.checking') : $t('mods.installed.checkUpdates')}
    </button>
    <button
      type="button"
      class="btn-secondary btn-xs"
      disabled={graphLoading}
      onclick={onRecheckDeps}
    >
      {graphLoading ? $t('mods.installed.resolvingDeps') : $t('mods.installed.recheckDeps')}
    </button>
    {#if updateCount > 0}
      <button type="button" class="btn-warning btn-xs" disabled={busy} onclick={onUpdateAll}>
        {$t('mods.installed.updateAll', { count: updateCount })}
      </button>
    {/if}
  </div>
  {#if counts.total > 0}
    <div
      role="radiogroup"
      aria-label={$t('mods.installed.filterGroupAriaLabel')}
      tabindex={-1}
      class="flex gap-1 text-xs"
      onkeydown={handleFilterKey}
    >
      <button
        type="button"
        role="radio"
        aria-checked={enabledFilter === 'all'}
        tabindex={enabledFilter === 'all' ? 0 : -1}
        data-value="all"
        class="btn-secondary btn-xs"
        class:bg-accent-soft={enabledFilter === 'all'}
        class:text-accent={enabledFilter === 'all'}
        class:font-medium={enabledFilter === 'all'}
        onclick={() => (enabledFilter = 'all')}
      >
        {$t('mods.installed.filterAll', { count: counts.total })}
      </button>
      <button
        type="button"
        role="radio"
        aria-checked={enabledFilter === 'enabled'}
        tabindex={enabledFilter === 'enabled' ? 0 : -1}
        data-value="enabled"
        class="btn-secondary btn-xs"
        class:bg-success-bg={enabledFilter === 'enabled'}
        class:text-success={enabledFilter === 'enabled'}
        class:font-medium={enabledFilter === 'enabled'}
        onclick={() => (enabledFilter = 'enabled')}
      >
        {$t('mods.installed.filterEnabled', { count: counts.enabled })}
      </button>
      <button
        type="button"
        role="radio"
        aria-checked={enabledFilter === 'disabled'}
        tabindex={enabledFilter === 'disabled' ? 0 : -1}
        data-value="disabled"
        class="btn-secondary btn-xs"
        class:bg-subtle={enabledFilter === 'disabled'}
        class:text-secondary={enabledFilter === 'disabled'}
        class:font-medium={enabledFilter === 'disabled'}
        onclick={() => (enabledFilter = 'disabled')}
      >
        {$t('mods.installed.filterDisabled', { count: counts.disabled })}
      </button>
    </div>
  {/if}
  {#if counts.updates > 0 || counts.issues > 0}
    <div class="flex gap-1 text-xs mt-1">
      {#if counts.updates > 0}
        <button
          type="button"
          class="btn-secondary btn-xs"
          class:bg-warning-bg={quickFilter === 'updates'}
          class:text-warning-text={quickFilter === 'updates'}
          class:font-medium={quickFilter === 'updates'}
          aria-pressed={quickFilter === 'updates'}
          onclick={() => (quickFilter = quickFilter === 'updates' ? 'all' : 'updates')}
        >
          {$t('mods.installed.filterUpdates', { count: counts.updates })}
        </button>
      {/if}
      {#if counts.issues > 0}
        <button
          type="button"
          class="btn-secondary btn-xs"
          class:bg-danger-bg={quickFilter === 'issues'}
          class:text-danger={quickFilter === 'issues'}
          class:font-medium={quickFilter === 'issues'}
          aria-pressed={quickFilter === 'issues'}
          onclick={() => (quickFilter = quickFilter === 'issues' ? 'all' : 'issues')}
        >
          {$t('mods.installed.filterIssues', { count: counts.issues })}
        </button>
      {/if}
    </div>
  {/if}
</div>

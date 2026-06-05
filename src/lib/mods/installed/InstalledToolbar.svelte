<script lang="ts">
  import { t } from '$lib/i18n';
  import Select from '$lib/ui/Select.svelte';
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
    viewFilter: ViewFilter;
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
  ]);

  // WCAG radiogroup keyboard pattern. Arrow / Home / End moves selection within
  // the group; the newly-checked radio gets focus. Roving tabindex (0 on
  // checked, -1 elsewhere) keeps the whole group as one tab stop.
  function handleFilterKey(e: KeyboardEvent) {
    const values = filterOptions.map((o) => o.value);
    const i = values.indexOf(viewFilter);
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
          class={`btn-secondary btn-xs ${viewFilter === opt.value ? opt.activeClass : ''}`}
          onclick={() => (viewFilter = opt.value)}
        >
          {opt.label}
        </button>
      {/each}
    </div>
  {/if}
</div>

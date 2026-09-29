<script lang="ts">
  import { t } from '$lib/i18n';
  import { relativeTime } from '$lib/format/relative-time';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import Select from '$lib/ui/Select.svelte';
  import OverflowMenu from '$lib/ui/OverflowMenu.svelte';
  import type { ContextMenuItem } from '$lib/ui/menu-item';
  import ToggleChipGroup from '$lib/ui/ToggleChipGroup.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { scrollRow } from '$lib/ui/scroll-row';
  import { stickyEdge } from '$lib/ui/sticky-edge';
  import type { SortBy, ViewFilter } from './installed-filters.svelte';

  let {
    counts,
    filter = $bindable(),
    sortBy = $bindable(),
    viewFilter = $bindable(),
    busy,
    checking,
    updateCount,
    checkedAtMs = null,
    rechecking,
    onCheckUpdates,
    onUpdateAll,
    onRecheckAll,
    onOpenModsFolder,
    issuesTone = 'danger',
  }: {
    counts: {
      total: number;
      enabled: number;
      disabled: number;
      updates: number;
      issues: number;
      needed: number;
      unusedLibraries: number;
    };
    filter: string;
    sortBy: SortBy;
    viewFilter: ViewFilter;
    busy: boolean;
    checking: boolean;
    updateCount: number;
    // When the persisted update check ran (unix ms), or null when none did.
    checkedAtMs?: number | null;
    /** The compat live check, the graph reload or the pre-flight run is in flight. */
    rechecking: boolean;
    onCheckUpdates: () => void;
    // Opens the review of the pending updates; the review runs them.
    onUpdateAll: () => void;
    onRecheckAll: () => void;
    onOpenModsFolder: () => void;
    // Danger while any mod blocks the launch, amber when only warnings remain
    // (spec D6: red means "the game won't start" and nothing else).
    issuesTone?: 'danger' | 'warning';
  } = $props();

  // «проверено …» moves on once a minute with no other change to re-render it.
  let now = $state(Date.now());
  $effect(() => {
    if (checkedAtMs === null) return;
    now = Date.now();
    const tick = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(tick);
  });

  const checkDisabledReason = $derived(
    counts.total === 0
      ? $t('mods.installed.disabledNoMods')
      : busy
        ? $t('mods.installed.disabledBusy')
        : '',
  );

  // The rare actions (spec D7). Re-check is the compat live check + graph + pre-flight in one;
  // «Открыть папку модов» mirrors the Overview control and reuses its key.
  const moreItems = $derived<ContextMenuItem[]>([
    {
      label: $t('mods.installed.recheckAll'),
      icon: 'refresh',
      disabled: rechecking || counts.total === 0,
      disabledReason: rechecking
        ? $t('mods.installed.rechecking')
        : $t('mods.installed.disabledNoMods'),
      onSelect: onRecheckAll,
      testId: 'installed-recheck-all',
    },
    {
      label: $t('instance.menu.openModsFolder'),
      icon: 'folderOpen',
      onSelect: onOpenModsFolder,
      testId: 'installed-open-mods-folder',
    },
  ]);

  const sortOptions = $derived([
    { value: 'name-asc', label: $t('mods.installed.sortNameAsc') },
    { value: 'name-desc', label: $t('mods.installed.sortNameDesc') },
    { value: 'recent', label: $t('mods.installed.sortRecent') },
    { value: 'source', label: $t('mods.installed.sortSource') },
  ]);

  // One mutually-exclusive filter group. All / Enabled / Disabled are always
  // present; Updates / Issues / Needed by others / Unused libraries appear only
  // when there is something to show (the graph views only once the dependency
  // graph has loaded). Each option carries its own tone so the chips read as
  // distinct kinds (state vs status vs graph) while behaving as a single
  // pick-one set. The count rides the chip's count badge; the label stays the
  // plain word so the accessible name matches the simple /All/ etc. patterns
  // the tests use.
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
    // Red while a mod stops the game — on its icon and count even when not chosen (attention, not
    // selection: DESIGN.md §6) — with the ✕ every blocking reason carries; the triangle is amber's.
    ...(counts.issues > 0
      ? [
          {
            value: 'issues',
            label: $t('mods.installed.filterIssuesLabel'),
            tone: issuesTone,
            icon: issuesTone === 'danger' ? ('circleX' as const) : ('warning' as const),
            count: counts.issues,
            testId: 'installed-filter-issues',
            attention: issuesTone === 'danger',
          },
        ]
      : []),
    ...(counts.needed > 0
      ? [
          {
            value: 'needed',
            label: $t('mods.installed.filterNeededLabel'),
            tone: 'accent' as const,
            count: counts.needed,
            testId: 'installed-filter-needed',
          },
        ]
      : []),
    ...(counts.unusedLibraries > 0
      ? [
          {
            value: 'unusedLibraries',
            label: $t('mods.installed.filterUnusedLibrariesLabel'),
            tone: 'accent' as const,
            count: counts.unusedLibraries,
            testId: 'installed-filter-unused-libraries',
          },
        ]
      : []),
  ]);
</script>

<!-- Stays on screen with its chips while the list scrolls (spec §6.7): on the page background,
     edge to edge over the view's padding, above the rows — they are positioned (accent strip,
     dependency ring) and would otherwise paint over it. The counts line is gone: the chips
     carry every count. It reserves its height in the scroll container, so a focused row never
     hides under it (`stickyEdge`, WCAG 2.4.11), and shows its bottom edge while it is stuck —
     rows cut under it read as scrolled, not broken. The edge is there at rest, transparent, so
     it appearing moves nothing.
     Compact in a narrow window (plan §5c V3: four lines, 163 px, at the default 820 px): the chips
     keep to one line that scrolls sideways, and the controls pack tighter — the sort drops its
     visible label at 1100 px and narrower (the Select keeps it as its name, and its value reads as a sort),
     and the update check never parts from its «checked …».
     Its z-10 makes it a stacking context, which holds the ⋯ menu, its click-scrim and the sort
     list at 10 too — under the sticky pager after the list, which then took the click meant to
     close the menu (plan §5d). While one of them is open (its trigger says `aria-expanded`), the
     bar lifts itself to the popover tier (DESIGN.md §14). -->
<div
  class="sticky top-0 z-10 has-[[aria-expanded=true]]:z-[var(--z-popover)] -mx-3 mb-2 flex flex-col gap-2 border-b border-transparent bg-base px-3 pt-1 pb-2 data-[stuck]:border-border-subtle"
  data-testid="installed-toolbar"
  use:stickyEdge
>
  <div class="flex flex-wrap gap-2 items-center">
    <input
      type="search"
      placeholder={$t('mods.installed.filterPlaceholder')}
      aria-label={$t('mods.installed.filterAriaLabel')}
      class="min-w-40 flex-1 border border-border-emphasis rounded px-3 py-1.5 text-sm"
      bind:value={filter}
    />
    <div class="text-xs text-secondary inline-flex items-center gap-1">
      <span class="max-[1100px]:hidden">{$t('mods.installed.sortLabel')}</span>
      <Select
        class="text-xs"
        ariaLabel={$t('mods.installed.sortLabel')}
        value={sortBy}
        options={sortOptions}
        onChange={(v) => (sortBy = String(v) as SortBy)}
      />
    </div>
    <span class="inline-flex items-center gap-2 whitespace-nowrap">
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
      {#if checkedAtMs !== null && !checking}
        <!-- A clock a moment behind the check still says "just now", never "−3s ago". -->
        <span class="text-xs text-muted" data-testid="updates-checked-at"
          >{$t('mods.updates.checkedAt', {
            when: relativeTime($t, checkedAtMs, Math.max(now, checkedAtMs)),
          })}</span
        >
      {/if}
    </span>
    {#if updateCount > 0}
      <!-- Only opens the review (D9): the review runs the updates and shows their spinner. -->
      <button
        type="button"
        class="btn-warning btn-xs whitespace-nowrap"
        disabled={busy}
        onclick={onUpdateAll}
      >
        {$t('mods.installed.updateAll', { count: updateCount })}
      </button>
    {/if}
    {#if rechecking}
      <Spinner size="sm" labelPlacement="right" label={$t('mods.installed.rechecking')} />
    {/if}
    <OverflowMenu items={moreItems} ariaLabel={$t('mods.installed.moreActions')} />
  </div>
  {#if counts.total > 0}
    <!-- One line that scrolls sideways (`scrollRow`, DESIGN.md §6): a fade on the side that has
         more, a focused chip scrolled clear of it, the chosen one kept in view. The line keeps
         room for a focus ring, which the scrolling box would clip, and gives it back with negative
         margins, so it sits where the chips did (`.scroll-row`, app.css). -->
    <div
      class="scroll-row"
      data-testid="installed-filter-row"
      use:scrollRow={{ keepInView: '[aria-checked="true"]' }}
    >
      <ToggleChipGroup
        wrap={false}
        options={filterOptions}
        value={viewFilter}
        onChange={(v) => (viewFilter = v as ViewFilter)}
        ariaLabel={$t('mods.installed.filterGroupAriaLabel')}
      />
    </div>
  {/if}
</div>

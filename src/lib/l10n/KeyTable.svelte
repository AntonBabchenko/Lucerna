<script lang="ts">
  // The per-namespace key editor: search, a state filter with per-bucket
  // counts, pagination, and one KeyEditRow per visible key.
  //
  // A namespace can have thousands of keys, so this fetches lazily per
  // (instanceId, namespace, lang) — mirroring LocalizationModal's own
  // requestId-guarded load() — and paginates client-side rather than
  // rendering every row (no virtualised list exists in this codebase; see
  // PAGE_SIZES).
  //
  // A saved/cleared row is patched into `rows` in place rather than
  // triggering a full re-fetch: KeyEditRow already knows the exact resulting
  // state from the (key, sourceEn, value) it just sent — see its own doc
  // comment — and re-fetching thousands of rows on every keystroke-save would
  // be wasteful. The one thing a local patch can't fix up is the namespace's
  // aggregate coverage percentage shown in the modal's sidebar; `onOverrideSaved`
  // lets the modal refresh that cheaply (l10nCoverage is cache-hit-fast for an
  // otherwise-unchanged instance) without this component needing to know it exists.
  import { commands, type KeyRow } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Pagination from '$lib/ui/Pagination.svelte';
  import SegmentedControl from '$lib/ui/SegmentedControl.svelte';
  import ToggleChipGroup from '$lib/ui/ToggleChipGroup.svelte';
  import { PAGE_SIZES, type PageSize } from '$lib/mods/browser-prefs.svelte';
  import {
    countKeyStates,
    countOrigins,
    filterByOrigin,
    filterRows,
    type KeyFilter,
    type OriginFilter,
    visibleOriginFilters,
    visibleStateFilters,
  } from './key-rows';
  import KeyEditRow from './KeyEditRow.svelte';

  let {
    instanceId,
    namespace,
    lang,
    onOverrideSaved,
    reloadToken = 0,
  }: {
    instanceId: string;
    namespace: string;
    lang: string;
    onOverrideSaved?: () => void;
    /**
     * Bump to force a refetch of the SAME (instance, namespace, lang). The
     * fetch effect is keyed on those three, so a change made outside this
     * component — an AI pre-fill run rewriting these very rows, a bulk revert
     * — is invisible to it: the rows on screen would stay pre-run until the
     * modal was reopened. Per-row edits do not need this; they patch in place.
     */
    reloadToken?: number;
  } = $props();

  let rows = $state<KeyRow[]>([]);
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let search = $state('');
  let filter = $state<KeyFilter>('all');
  // A SECOND axis, deliberately not folded into `filter`: ToggleChipGroup is a
  // single-select radiogroup with one `value`, so putting manual/machine in
  // the same group would make "untranslated" and "machine-written" mutually
  // exclusive — and they are orthogonal.
  let originFilter = $state<OriginFilter>('all');
  let page = $state(0);
  let pageSize = $state<PageSize>(50);
  let revertConfirmOpen = $state(false);
  let reverting = $state(false);
  let revertError = $state<string | null>(null);

  // Monotonic request id — same race guard as LocalizationModal.load(): a
  // response only applies if it's still the most recent request for this
  // table (namespace/lang can change quickly while a fetch is in flight).
  let requestId = 0;

  async function load(id: string, ns: string, targetLang: string) {
    const myRequest = ++requestId;
    loading = true;
    loadError = null;
    const res = await commands.l10nNamespaceKeys(id, ns, targetLang);
    if (myRequest !== requestId) return;
    loading = false;
    if (res.status === 'ok') {
      rows = res.data;
    } else {
      rows = [];
      loadError = $t('instance.l10n.keyTable.loadFailed', { error: formatError(res.error) });
    }
  }

  $effect(() => {
    const id = instanceId;
    const ns = namespace;
    const targetLang = lang;
    void reloadToken;
    void load(id, ns, targetLang);
  });

  // A new namespace is an unrelated key set — carrying over a search term or
  // filter from the last one would just hide everything. A language switch
  // on the SAME namespace keeps them: same keys, same reason to be searching.
  $effect(() => {
    void namespace;
    search = '';
    filter = 'all';
    // The origin filter resets with the others: a "machine only" selection
    // carried into a namespace the pre-fill never touched shows an empty
    // table with no visible reason why.
    originFilter = 'all';
    // Defence in depth. ConfirmDialog's backdrop covers the namespace list, so
    // a switch while the confirm is up should be unreachable today — but if it
    // ever isn't, the confirm would name one namespace and revert another.
    revertConfirmOpen = false;
    revertError = null;
  });

  const filteredRows = $derived(filterByOrigin(filterRows(rows, search, filter), originFilter));
  const counts = $derived(countKeyStates(rows));
  const origins = $derived(countOrigins(rows));
  const pageCount = $derived(Math.max(1, Math.ceil(filteredRows.length / pageSize)));
  const paged = $derived(filteredRows.slice(page * pageSize, page * pageSize + pageSize));

  // The empty state has two causes and only ever named one of them.
  const emptyMessage = $derived(
    search.trim() === ''
      ? $t('instance.l10n.keyTable.noResultsFilter')
      : $t('instance.l10n.keyTable.noResults'),
  );

  // Reset to page 0 whenever the visible set's shape changes; clamp down if
  // it shrinks (e.g. switching to a near-empty filter while on a later page).
  $effect(() => {
    void search;
    void filter;
    void originFilter;
    void pageSize;
    page = 0;
  });
  $effect(() => {
    if (page > pageCount - 1) page = Math.max(0, pageCount - 1);
  });

  function patchRow(updated: KeyRow) {
    rows = rows.map((r) => (r.key === updated.key ? updated : r));
    onOverrideSaved?.();
  }

  const allFilterOptions = $derived([
    {
      value: 'all',
      label: $t('instance.l10n.keyTable.filterAllLabel'),
      tone: 'neutral' as const,
      count: counts.all,
      testId: 'l10n-filter-all',
    },
    {
      value: 'translated',
      label: $t('instance.l10n.keyTable.filterTranslatedLabel'),
      tone: 'success' as const,
      count: counts.translated,
      testId: 'l10n-filter-translated',
    },
    {
      value: 'missing',
      label: $t('instance.l10n.keyTable.filterMissingLabel'),
      tone: 'muted' as const,
      count: counts.missing,
      testId: 'l10n-filter-missing',
    },
    {
      value: 'stale',
      label: $t('instance.l10n.keyTable.filterStaleLabel'),
      tone: 'warning' as const,
      count: counts.stale,
      testId: 'l10n-filter-stale',
    },
    {
      value: 'orphan',
      label: $t('instance.l10n.keyTable.filterOrphanLabel'),
      tone: 'danger' as const,
      count: counts.orphan,
      testId: 'l10n-filter-orphan',
    },
  ]);
  const filterOptions = $derived.by(() => {
    const visible = new Set<string>(visibleStateFilters(counts));
    return allFilterOptions.filter((o) => visible.has(o.value));
  });

  const allOriginOptions = $derived([
    {
      value: 'all',
      label: $t('instance.l10n.keyTable.originAllLabel'),
      tone: 'neutral' as const,
      testId: 'l10n-origin-all',
    },
    {
      value: 'manual',
      label: $t('instance.l10n.keyTable.originManualLabel'),
      tone: 'neutral' as const,
      count: origins.manual,
      testId: 'l10n-origin-manual',
    },
    {
      value: 'machine',
      label: $t('instance.l10n.keyTable.originMachineLabel'),
      tone: 'neutral' as const,
      count: origins.machine,
      testId: 'l10n-origin-machine',
    },
  ]);
  const originOptions = $derived.by(() => {
    const visible = new Set<string>(visibleOriginFilters(origins));
    return allOriginOptions.filter((o) => visible.has(o.value));
  });

  // A chip can vanish under an active selection (the last stale key was
  // fixed, a revert removed every machine string). Falling back to 'all'
  // keeps the table from showing an empty list under an invisible filter.
  $effect(() => {
    if (!filterOptions.some((o) => o.value === filter)) filter = 'all';
  });
  $effect(() => {
    if (!originOptions.some((o) => o.value === originFilter)) originFilter = 'all';
  });

  // One command, not one call per key: the backend loads the namespace file
  // once, drops every Origin::Machine entry, saves once and rebuilds the pack.
  // A hand-edited machine string is already Origin::Manual (KeyEditRow's save
  // reclaims it), so this only ever removes what the user never touched.
  async function revertMachine() {
    if (reverting) return;
    reverting = true;
    revertError = null;
    try {
      const res = await commands.l10nRevertMachine(instanceId, lang, namespace);
      if (res.status === 'ok') {
        revertConfirmOpen = false;
        // Refetch rather than patch: the removal is bulk and the resulting
        // state of each key depends on whether the mod ships its own
        // translation — exactly the thing the backend already knows.
        await load(instanceId, namespace, lang);
        onOverrideSaved?.();
      } else {
        revertError = $t('instance.l10n.keyTable.revertFailed', { error: formatError(res.error) });
      }
    } finally {
      reverting = false;
    }
  }

  const pageSizeOptions = PAGE_SIZES.map((n) => ({ value: String(n), label: String(n) }));
</script>

<div class="flex h-full min-h-0 flex-col">
  <div
    class="flex items-center gap-3 border-b border-border-subtle px-3 py-2"
    data-testid="l10n-pane-header"
  >
    <span class="truncate font-mono text-xs text-primary">{namespace}</span>
    <span class="shrink-0 text-xs text-muted">
      {$t('instance.l10n.keyTable.paneSummary', {
        total: counts.all,
        translated: counts.translated,
      })}
    </span>
    {#if origins.machine > 0}
      <!--
        Scoped to THIS namespace (revertMachine passes `namespace`), so it
        lives in the namespace's own header rather than among the filters,
        where it read as the undo of the header's instance-wide AI action.
      -->
      <button
        type="button"
        class="btn-ghost-danger ml-auto shrink-0"
        data-testid="l10n-revert-machine"
        onclick={() => {
          revertError = null;
          revertConfirmOpen = true;
        }}
      >
        {$t('instance.l10n.keyTable.revertMachineButton')}
      </button>
    {/if}
  </div>

  <div class="flex flex-col gap-2 border-b border-border-subtle px-3 py-2">
    <input
      type="search"
      class="filter-control w-full"
      placeholder={$t('instance.l10n.keyTable.searchPlaceholder')}
      aria-label={$t('instance.l10n.keyTable.searchAriaLabel')}
      data-testid="l10n-key-search"
      bind:value={search}
    />
    <div class="flex flex-wrap items-center gap-2">
      <span class="text-xs uppercase tracking-wide text-muted">
        {$t('instance.l10n.keyTable.filterGroupLabel')}
      </span>
      <ToggleChipGroup
        options={filterOptions}
        value={filter}
        onChange={(v) => (filter = v as KeyFilter)}
        ariaLabel={$t('instance.l10n.keyTable.filterGroupAriaLabel')}
      />
      <span class="mx-1 h-5 w-px shrink-0 bg-border-subtle"></span>
      <!-- Its own group with its own ariaLabel — see `originFilter` above. -->
      <span class="text-xs uppercase tracking-wide text-muted">
        {$t('instance.l10n.keyTable.originGroupLabel')}
      </span>
      <ToggleChipGroup
        options={originOptions}
        value={originFilter}
        onChange={(v) => (originFilter = v as OriginFilter)}
        ariaLabel={$t('instance.l10n.keyTable.originGroupAriaLabel')}
      />
    </div>
  </div>

  <div class="flex-1 overflow-y-auto px-3">
    {#if loading}
      <LoadingPanel label={$t('instance.l10n.keyTable.loading')} />
    {:else if loadError}
      <p class="p-3 text-sm text-danger" data-testid="l10n-key-table-error">{loadError}</p>
    {:else if paged.length === 0}
      <p class="p-3 text-sm text-muted" data-testid="l10n-key-table-empty">
        {emptyMessage}
      </p>
    {:else}
      <!--
        Keyed by (namespace, lang, row.key), not just row.key: translation keys
        are identical across languages, so a bare row.key key would let Svelte
        reuse the same KeyEditRow instance across a language switch. KeyEditRow
        seeds its `draft` state from `row` ONCE, at creation, and never re-syncs
        it (see its own doc comment) — the previous language's text would
        linger in the input, read as an unsaved edit against the new language's
        value, and a Save would write it under the new language.

        `load()` below also happens to make this unreachable in practice right
        now: it sets `loading = true` synchronously before every `await`, which
        hides this entire {#each} behind the {#if loading} branch above and
        tears it down — Promise continuations are always microtask-deferred, so
        that commit is guaranteed, not a timing accident (verified empirically:
        the row's DOM node identity changes across a language switch even with
        a bare `row.key` key — see the "recreates the row instance" test in
        tests/l10n-key-table.test.ts). That protection is incidental, not
        structural: a future "don't flash a spinner over content that's still
        valid while only the language changes" optimization could drop the
        teardown and silently reintroduce the bug. Keying on the full
        (namespace, lang, key) triple makes the invariant hold on its own terms
        — a row belongs to exactly one such pair and is destroyed and recreated
        the instant any of the three changes — independent of whatever loading
        UX this component has today or grows tomorrow.
      -->
      {#each paged as row (`${namespace}|${lang}|${row.key}`)}
        <KeyEditRow {row} {namespace} {lang} onSaved={patchRow} />
      {/each}
    {/if}
  </div>

  {#if !loading && !loadError && filteredRows.length > 0}
    <div class="border-t border-border-subtle px-3">
      <Pagination {page} {pageCount} onPage={(n) => (page = n)}>
        {#snippet end()}
          <span class="inline-flex items-center gap-2 text-sm">
            <span class="text-muted">{$t('instance.l10n.keyTable.perPage')}</span>
            <SegmentedControl
              options={pageSizeOptions}
              value={String(pageSize)}
              onChange={(v) => (pageSize = Number(v) as PageSize)}
              variant="inline"
              ariaLabel={$t('instance.l10n.keyTable.perPage')}
            />
          </span>
        {/snippet}
      </Pagination>
    </div>
  {/if}
</div>

{#if revertConfirmOpen}
  <ConfirmDialog
    title={$t('instance.l10n.keyTable.revertConfirmTitle')}
    bodyText={$t('instance.l10n.keyTable.revertConfirmBody', {
      count: origins.machine,
      namespace,
    })}
    confirmLabel={$t('instance.l10n.keyTable.revertConfirmButton')}
    variant="danger"
    busy={reverting}
    error={revertError}
    confirmTestid="l10n-revert-confirm"
    onCancel={() => (revertConfirmOpen = false)}
    onConfirm={revertMachine}
  />
{/if}

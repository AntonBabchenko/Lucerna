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
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Pagination from '$lib/ui/Pagination.svelte';
  import SegmentedControl from '$lib/ui/SegmentedControl.svelte';
  import ToggleChipGroup from '$lib/ui/ToggleChipGroup.svelte';
  import { PAGE_SIZES, type PageSize } from '$lib/mods/browser-prefs.svelte';
  import { countKeyStates, filterRows, type KeyFilter } from './key-rows';
  import KeyEditRow from './KeyEditRow.svelte';

  let {
    instanceId,
    namespace,
    lang,
    onOverrideSaved,
  }: {
    instanceId: string;
    namespace: string;
    lang: string;
    onOverrideSaved?: () => void;
  } = $props();

  let rows = $state<KeyRow[]>([]);
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let search = $state('');
  let filter = $state<KeyFilter>('all');
  let page = $state(0);
  let pageSize = $state<PageSize>(50);

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
    void load(id, ns, targetLang);
  });

  // A new namespace is an unrelated key set — carrying over a search term or
  // filter from the last one would just hide everything. A language switch
  // on the SAME namespace keeps them: same keys, same reason to be searching.
  $effect(() => {
    void namespace;
    search = '';
    filter = 'all';
  });

  const filteredRows = $derived(filterRows(rows, search, filter));
  const counts = $derived(countKeyStates(rows));
  const pageCount = $derived(Math.max(1, Math.ceil(filteredRows.length / pageSize)));
  const paged = $derived(filteredRows.slice(page * pageSize, page * pageSize + pageSize));

  // Reset to page 0 whenever the visible set's shape changes; clamp down if
  // it shrinks (e.g. switching to a near-empty filter while on a later page).
  $effect(() => {
    void search;
    void filter;
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

  const filterOptions = $derived([
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

  const pageSizeOptions = PAGE_SIZES.map((n) => ({ value: String(n), label: String(n) }));
</script>

<div class="flex h-full min-h-0 flex-col">
  <div class="flex flex-wrap items-center gap-2 border-b border-border-subtle px-3 py-2">
    <input
      type="search"
      class="h-8 min-w-[10rem] flex-1 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
      placeholder={$t('instance.l10n.keyTable.searchPlaceholder')}
      aria-label={$t('instance.l10n.keyTable.searchAriaLabel')}
      data-testid="l10n-key-search"
      bind:value={search}
    />
    <ToggleChipGroup
      options={filterOptions}
      value={filter}
      onChange={(v) => (filter = v as KeyFilter)}
      ariaLabel={$t('instance.l10n.keyTable.filterGroupAriaLabel')}
    />
  </div>

  <div class="flex-1 overflow-y-auto px-3">
    {#if loading}
      <LoadingPanel label={$t('instance.l10n.keyTable.loading')} />
    {:else if loadError}
      <p class="p-3 text-sm text-danger" data-testid="l10n-key-table-error">{loadError}</p>
    {:else if paged.length === 0}
      <p class="p-3 text-sm text-muted" data-testid="l10n-key-table-empty">
        {$t('instance.l10n.keyTable.noResults')}
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

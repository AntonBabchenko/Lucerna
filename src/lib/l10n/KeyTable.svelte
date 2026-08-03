<script lang="ts">
  // The per-namespace key editor: search, a single view filter with per-bucket
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
  import { SvelteSet } from 'svelte/reactivity';
  import type { ToggleChipTone } from '$lib/ui/ToggleChip.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import {
    countKeyStates,
    countOrigins,
    filterRows,
    type KeyView,
    stickyOutOfView,
    viewCount,
    visibleViews,
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
     * component — an AI pre-fill run rewriting these very rows — is invisible
     * to it: the rows on screen would stay pre-run until the modal was
     * reopened. Per-row edits do not need this; they patch in place, and the
     * bulk revert calls load() directly.
     */
    reloadToken?: number;
  } = $props();

  let rows = $state<KeyRow[]>([]);
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let search = $state('');
  let view = $state<KeyView>('all');
  // Keys the user has changed in this sitting. A changed row keeps its place
  // in the list instead of being yanked out from under the cursor — see
  // filterRows. Keys, never KeyRow objects: `rows` is patched in place on
  // every save, so a snapshot taken here would go stale against the row it
  // names and feed old text to a KeyEditRow that never re-syncs. The sharper
  // version of that hazard — the bulk revert deleting a row outright, a
  // snapshot resurrecting it under an unchanged each-key — is what motivated
  // the `sticky.clear()` in load(), which now forecloses it.
  const sticky = new SvelteSet<string>();
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
    // Every full row replacement passes through here — the fetch effect
    // (namespace / language / reloadToken) and the bulk revert, which calls
    // load() directly and never re-runs that effect.
    //
    // Cleared here rather than before the await: the row list is hidden while
    // `loading` is true, but the toolbar is not, and clearing early would drop
    // `keepView` while `counts` still described the OLD rows — resetting the
    // user's view to All mid-fetch, permanently. Placing it after the race
    // guard also means a superseded response leaves the set alone; the request
    // that wins will clear it.
    sticky.clear();
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
  //
  // Guarded on the VALUE, not merely on the effect re-running. This effect
  // writes user-visible toolbar state, so it must fire on a namespace change
  // and on nothing else — but an effect that only reads a prop also re-runs
  // whenever that prop's signal is invalidated with an unchanged value. A
  // `reloadToken` bump is exactly such an occasion under any parent that hands
  // props over as one object rather than per-key getters; it is what
  // @testing-library/svelte's rerender() does (it reassigns a single
  // `$state.raw` props object — see svelte-core/src/props.svelte.js), so
  // without this guard an external refetch silently threw the user back to
  // "All" with their search box emptied. `lastNamespace` is a plain `let`, not
  // `$state`: the effect writes it, and tracking it would only make the effect
  // invalidate itself.
  let lastNamespace: string | null = null;
  $effect(() => {
    if (namespace === lastNamespace) return;
    lastNamespace = namespace;
    search = '';
    view = 'all';
    // Defence in depth. ConfirmDialog's backdrop covers the namespace list, so
    // a switch while the confirm is up should be unreachable today — but if it
    // ever isn't, the confirm would name one namespace and revert another.
    revertConfirmOpen = false;
    revertError = null;
  });

  const filteredRows = $derived(filterRows(rows, search, view, sticky));
  const counts = $derived(countKeyStates(rows));
  const origins = $derived(countOrigins(rows));
  // Sticky rows are folded into `filteredRows`, not appended to `paged`,
  // precisely so pageCount / showPagination / the clamp below all describe
  // what is on screen. Folded in, a save can never shrink the set, so it can
  // never bounce the user back a page or make the footer disappear.
  // Takes `search` as well as `view`: a sticky row the search excludes is not
  // on screen either, so counting it would offer to refresh away a row the
  // user cannot see. This is exactly the count of rendered rows that
  // `filterRows` would stop returning, under an unchanged view, if the set
  // were cleared.
  const stickyShown = $derived(stickyOutOfView(rows, search, view, sticky));
  const pageCount = $derived(Math.max(1, Math.ceil(filteredRows.length / pageSize)));
  const paged = $derived(filteredRows.slice(page * pageSize, page * pageSize + pageSize));

  // Below the smallest page size there is exactly one page AND the per-page
  // picker cannot change anything, so the whole footer is chrome. The shared
  // Pagination deliberately never self-hides — the threshold is this
  // surface's call, because a near-empty namespace is normal here.
  const showPagination = $derived(filteredRows.length > PAGE_SIZES[0]);

  // Three causes, not two: a search term, a view that excludes everything, or
  // a namespace with no keys at all. Blaming "the filter" when the user is
  // standing in All and the mod simply ships nothing sends them looking for a
  // control to clear that is already cleared.
  const emptyMessage = $derived(
    search.trim() !== ''
      ? $t('instance.l10n.keyTable.noResults')
      : view !== 'all'
        ? $t('instance.l10n.keyTable.noResultsFilter')
        : $t('instance.l10n.keyTable.noResultsEmpty'),
  );

  // Reset to page 0 whenever the visible set's shape changes; clamp down if
  // it shrinks (e.g. switching to a near-empty filter while on a later page).
  //
  // `namespace` is read explicitly rather than relying on the reset effect
  // above: those writes are `===` no-ops whenever the toolbar is already at
  // its defaults, so without this a user on page 3 of one mod lands on page 3
  // of the next one. The clamp below only ever clamps downward, so it cannot
  // stand in for this.
  $effect(() => {
    void namespace;
    void search;
    void view;
    void pageSize;
    page = 0;
  });
  $effect(() => {
    if (page > pageCount - 1) page = Math.max(0, pageCount - 1);
  });

  function patchRow(updated: KeyRow) {
    const previous = rows.find((r) => r.key === updated.key);
    // Clearing an orphan's override does not change the row, it deletes it.
    // An orphan exists only because it has an override: `state_of` reaches
    // Orphan inside `if let Some(entry)`, and the backend's key universe is
    // `en.keys() ∪ store.entries.keys()` — with the entry gone the key is in
    // neither, so the next fetch returns no such row. Any patched state would
    // be a lie, and 'orphan' specifically would break the invariant that an
    // override and an origin arrive together.
    if (previous?.state === 'orphan' && updated.overrideValue === null) {
      rows = rows.filter((r) => r.key !== updated.key);
      sticky.delete(updated.key);
      onOverrideSaved?.();
      return;
    }
    rows = rows.map((r) => (r.key === updated.key ? updated : r));
    // Both a save and a clear land here, and both make the row sticky. A
    // cleared row usually re-enters the active view on its own, in which case
    // `stickyOutOfView` ignores it and the exemption is inert — so one rule
    // covers both: a row you changed does not move until you ask for a fresh
    // list.
    sticky.add(updated.key);
    onOverrideSaved?.();
  }

  // Colour encodes attention, not decoration (DESIGN.md §9): each state view
  // carries the tone of what it means — success for done, muted for not
  // started, warning and danger for the two that need looking at. 'all' and
  // the two origin views are not states of their own; they slice across the
  // others, so they stay neutral rather than competing with them.
  const VIEW_TONE: Record<KeyView, ToggleChipTone> = {
    all: 'neutral',
    translated: 'success',
    missing: 'muted',
    stale: 'warning',
    orphan: 'danger',
    manual: 'neutral',
    machine: 'neutral',
  };

  // Annotated rather than `$derived<...>`, matching PrefillDialog's
  // PHASE_LABELS next door.
  const viewLabels: Record<KeyView, string> = $derived({
    all: $t('instance.l10n.keyTable.filterAllLabel'),
    translated: $t('instance.l10n.keyTable.filterTranslatedLabel'),
    missing: $t('instance.l10n.keyTable.filterMissingLabel'),
    stale: $t('instance.l10n.keyTable.filterStaleLabel'),
    orphan: $t('instance.l10n.keyTable.filterOrphanLabel'),
    manual: $t('instance.l10n.keyTable.originManualLabel'),
    machine: $t('instance.l10n.keyTable.originMachineLabel'),
  });

  // `view` unconditionally, not "only while it holds sticky rows": the chip you
  // are standing on must never vanish under you. Gated on stickiness, clicking
  // Refresh — the button whose entire job is to RELEASE the hold — dropped a
  // zero-count chip and let the fallback below dump the user into All.
  const viewOptions = $derived(
    visibleViews(counts, origins, view).map((v) => ({
      value: v,
      label: viewLabels[v],
      tone: VIEW_TONE[v],
      count: viewCount(v, counts, origins),
      testId: `l10n-filter-${v}`,
    })),
  );

  // Unreachable by construction: `viewOptions` passes `view` as keepView, so
  // the active view is always among the rendered chips. Retained as defence in
  // depth because the failure it guards is not cosmetic — an unresolvable
  // `view` makes the whole chip group unreachable by keyboard, since
  // ToggleChipGroup gives the tab stop only to the active option and
  // nextRovingIndex refuses to move from an unresolved index.
  $effect(() => {
    if (!viewOptions.some((o) => o.value === view)) view = 'all';
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
    <!--
      Both of these describe `rows`, which still holds the PREVIOUS namespace
      until the in-flight fetch lands — while `namespace` above is already the
      new one. Rendering them during the load would put one namespace's name
      beside another's counts, and the revert confirm interpolates both, so it
      could promise to remove a count that belongs to the namespace the user
      just left.
    -->
    {#if !loading}
      <span class="shrink-0 text-xs text-muted">
        {$t('instance.l10n.keyTable.paneSummary', {
          total: counts.all,
          translated: counts.translated,
        })}
      </span>
    {/if}
    {#if !loading && origins.machine > 0}
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
      <!--
        One group, not two. See KeyView in key-rows.ts for why origin cannot be
        a second axis. No visible caption: every other chip group in the app
        (InstalledToolbar, JournalPanel, LogsPopover, the server panes) names
        itself with ariaLabel alone, and "State" would be false for a list that
        contains Manual and AI.
      -->
      <ToggleChipGroup
        options={viewOptions}
        value={view}
        onChange={(v) => {
          view = v as KeyView;
          // Picking a view IS asking for a fresh list. onChange fires even for
          // the already-active chip (the assignment above is then a no-op), so
          // re-clicking it is a second, discoverable way to re-run the filter.
          // The automatic fallback above writes `view` directly and so stays
          // exempt, which is the point: it is not a user action.
          sticky.clear();
        }}
        ariaLabel={$t('instance.l10n.keyTable.filterGroupAriaLabel')}
      />
      {#if stickyShown > 0}
        <button
          type="button"
          class="btn-ghost ml-auto shrink-0"
          data-testid="l10n-sticky-refresh"
          use:tooltip={$t('instance.l10n.keyTable.stickyRefreshTitle')}
          onclick={() => sticky.clear()}
        >
          {$t('instance.l10n.keyTable.stickyRefreshButton', { count: stickyShown })}
        </button>
      {/if}
    </div>
  </div>

  <div class="flex-1 overflow-y-auto px-3">
    {#if loading}
      <LoadingPanel label={$t('instance.l10n.keyTable.loading')} />
    {:else if loadError}
      <p role="alert" class="p-3 text-sm text-danger" data-testid="l10n-key-table-error">
        {loadError}
      </p>
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

  {#if !loading && !loadError && showPagination}
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

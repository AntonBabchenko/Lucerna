<script lang="ts">
  // Instance-wide search results: the answer to "I saw this line in game and I
  // don't know which mod it's from".
  //
  // Rows are the SAME `KeyEditRow` the per-mod editor uses, so a hit is
  // editable where it stands — the whole point is not having to find the mod
  // first — and a searched row can never drift from a browsed one.
  //
  // Every row carries its namespace, because that is precisely the fact the
  // user came here missing.
  import { commands, type SearchResult } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import KeyEditRow from './KeyEditRow.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';

  let {
    instanceId,
    lang,
    query,
    onSaved,
  }: {
    instanceId: string;
    lang: string;
    /** Already debounced by the caller: this component must not re-run the jar
     *  walk on every keystroke. */
    query: string;
    onSaved: () => void;
  } = $props();

  let result = $state<SearchResult | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  // Monotonic, mirroring LocalizationModal's own guard: a slow answer for a
  // query the user has since retyped must not overwrite a newer one.
  let requestId = 0;

  $effect(() => {
    const id = instanceId;
    const targetLang = lang;
    const q = query;
    const mine = ++requestId;
    if (q.trim() === '') {
      result = null;
      loading = false;
      error = null;
      return;
    }
    loading = true;
    error = null;
    void commands.l10nSearch(id, targetLang, q).then((res) => {
      if (mine !== requestId) return;
      loading = false;
      if (res.status === 'ok') {
        result = res.data;
      } else {
        result = null;
        error = formatError(res.error);
      }
    });
  });
</script>

{#if loading && !result}
  <LoadingPanel label={$t('common.loading')} />
{:else if error}
  <p class="p-4 text-sm text-danger" role="alert" data-testid="l10n-find-error">{error}</p>
{:else if result}
  {#if result.hits.length === 0}
    <!--
      The failure this whole feature exists to avoid is a bare "nothing found":
      the user concludes the launcher is broken and stops. Two different causes,
      two different sentences — and the disabled-mods one is actionable.
    -->
    <div class="flex flex-col gap-2 p-4" data-testid="l10n-find-none">
      <p class="text-sm text-secondary">{$t('instance.l10n.find.noneBody')}</p>
      {#if result.disabledMods > 0}
        <p class="text-xs text-warning-text" data-testid="l10n-find-disabled-hint">
          {$t('instance.l10n.find.disabledHint', { count: result.disabledMods })}
        </p>
      {/if}
    </div>
  {:else}
    <div class="flex items-center justify-between gap-3 px-4 py-2">
      <span class="text-xs text-secondary" data-testid="l10n-find-count">
        {$t('instance.l10n.find.count', { count: result.hits.length })}
      </span>
      {#if result.truncated}
        <!-- Never a silent cap: a truncated list that claims to be whole is
             worse than one that admits it stopped. -->
        <span class="text-xs text-warning-text" data-testid="l10n-find-truncated">
          {$t('instance.l10n.find.truncated')}
        </span>
      {/if}
    </div>
    <ul class="flex flex-col">
      {#each result.hits as hit (`${hit.namespace}/${hit.row.key}`)}
        <li class="border-b px-4 py-2 last:border-b-0">
          <span
            class="mb-1 inline-block rounded bg-subtle px-1.5 py-0.5 text-xs text-secondary"
            data-testid={`l10n-find-hit-ns-${hit.namespace}`}
          >
            {hit.namespace}
          </span>
          <KeyEditRow row={hit.row} namespace={hit.namespace} {lang} onSaved={() => onSaved()} />
        </li>
      {/each}
    </ul>
    {#if result.disabledMods > 0}
      <!-- Shown even when there ARE hits: some of the matches may be sitting in
           the half of the pack that was never scanned. -->
      <p class="px-4 py-2 text-xs text-muted" data-testid="l10n-find-disabled-note">
        {$t('instance.l10n.find.disabledHint', { count: result.disabledMods })}
      </p>
    {/if}
  {/if}
{/if}

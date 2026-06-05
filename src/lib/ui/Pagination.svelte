<script lang="ts">
  import type { Snippet } from 'svelte';
  import { t } from '$lib/i18n';

  // Shared pagination control for every browser (mods / RP / shaders, modpacks,
  // installed). Index-based and presentational: `page` is 0-based, the component
  // emits the clamped target index via `onPage`; the container reloads. Layout
  // mirrors the Steam-style footer the browsers already used — leading spacer,
  // First · Prev · "N of M" · Next · Last, then an optional end slot (page-size
  // picker) pinned right.
  let {
    page,
    pageCount,
    onPage,
    disabled = false,
    end,
  }: {
    page: number;
    pageCount: number;
    onPage: (n: number) => void;
    disabled?: boolean;
    end?: Snippet;
  } = $props();

  // pageCount is always >= 1; the last reachable index is pageCount - 1.
  const lastIndex = $derived(Math.max(0, pageCount - 1));
  const atFirst = $derived(disabled || page <= 0);
  const atLast = $derived(disabled || page >= lastIndex);
</script>

<div class="flex items-center gap-2 text-sm text-secondary pt-2">
  <span class="flex-1"></span>
  <button
    type="button"
    class="btn-secondary btn-sm"
    data-testid="pg-first"
    disabled={atFirst}
    onclick={() => onPage(0)}
  >
    {$t('pagination.first')}
  </button>
  <button
    type="button"
    class="btn-secondary btn-sm"
    data-testid="pg-prev"
    disabled={atFirst}
    onclick={() => onPage(page - 1)}
  >
    {$t('pagination.prev')}
  </button>
  <span data-testid="pg-label">
    {$t('pagination.pageOf', { page: page + 1, total: pageCount })}
  </span>
  <button
    type="button"
    class="btn-secondary btn-sm"
    data-testid="pg-next"
    disabled={atLast}
    onclick={() => onPage(page + 1)}
  >
    {$t('pagination.next')}
  </button>
  <button
    type="button"
    class="btn-secondary btn-sm"
    data-testid="pg-last"
    disabled={atLast}
    onclick={() => onPage(lastIndex)}
  >
    {$t('pagination.last')}
  </button>
  <span class="flex-1 flex justify-end">
    {@render end?.()}
  </span>
</div>

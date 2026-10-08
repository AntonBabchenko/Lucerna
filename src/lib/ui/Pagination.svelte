<script lang="ts">
  import type { Snippet } from 'svelte';
  import { t } from '$lib/i18n';
  import { Icon, type IconName } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';

  // Shared pagination control for every browser (mods / RP / shaders, modpacks,
  // installed). Index-based and presentational: `page` is 0-based, the component
  // emits the clamped target index via `onPage`; the container reloads. Layout
  // mirrors the Steam-style footer the browsers already used — leading spacer,
  // « · ‹ · "N of M" · › · », then an optional end slot (page-size picker) pinned
  // right.
  //
  // The four steps are icon-only at every width, each named by a full phrase
  // ("Next page") that it also says in a tooltip (DESIGN.md §5). Labelled steps
  // did not fit the launcher's default 820 px window (654 px in a 556 px box, the
  // page-size picker off the window), and beside a Cyrillic label the |< and >|
  // glyphs read as the letter «К» — hence « and » for first and last. "N of M"
  // and the end slot never break inside themselves; where a box is narrower
  // still, the row wraps rather than overflows.
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

<!-- A step: its tooltip sits on a wrapper, since a disabled button fires no pointer events
     (DESIGN.md §5); the button keeps the same words as its aria-label. `p-2` makes it the
     square of a labelled `btn-sm` button's height. No `btn-sm` on it: `.btn-sm` is declared
     after the utilities in app.css, so its padding would win over `p-2`. -->
{#snippet step(testid: string, label: string, icon: IconName, off: boolean, target: number)}
  <span class="inline-flex" use:tooltip={{ text: label, describe: false }}>
    <button
      type="button"
      class="btn-secondary inline-flex items-center justify-center p-2"
      data-testid={testid}
      aria-label={label}
      disabled={off}
      onclick={() => onPage(target)}
    >
      <Icon name={icon} size={16} />
    </button>
  </span>
{/snippet}

<div class="flex flex-wrap items-center gap-2 text-sm text-secondary pt-2">
  <span class="flex-1"></span>
  {@render step('pg-first', $t('pagination.first'), 'chevronsLeft', atFirst, 0)}
  {@render step('pg-prev', $t('pagination.prev'), 'chevronLeft', atFirst, page - 1)}
  <span class="whitespace-nowrap" data-testid="pg-label">
    {$t('pagination.pageOf', { page: page + 1, total: pageCount })}
  </span>
  {@render step('pg-next', $t('pagination.next'), 'chevronRight', atLast, page + 1)}
  {@render step('pg-last', $t('pagination.last'), 'chevronsRight', atLast, lastIndex)}
  <span class="flex-1 flex justify-end">
    {@render end?.()}
  </span>
</div>

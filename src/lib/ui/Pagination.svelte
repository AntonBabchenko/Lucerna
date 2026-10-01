<script lang="ts">
  import type { Snippet } from 'svelte';
  import { t } from '$lib/i18n';
  import { Icon, type IconName } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';

  // Shared pagination control for every browser (mods / RP / shaders, modpacks,
  // installed). Index-based and presentational: `page` is 0-based, the component
  // emits the clamped target index via `onPage`; the container reloads. Layout
  // mirrors the Steam-style footer the browsers already used — leading spacer,
  // First · Prev · "N of M" · Next · Last, then an optional end slot (page-size
  // picker) pinned right.
  //
  // In a window narrower than 1100 px the four steps show their icons only: at the
  // launcher's default 820 px the labelled row was 654 px in a 556 px box, and the
  // page-size picker ended up off the window (plan §5e). From 1100 px on nothing
  // changes — `max-[1099px]:`, since Tailwind's `max-[Npx]` includes N itself. A
  // step keeps its label as its name — visually hidden, never removed — and says
  // it in a tooltip while it is hidden. "N of M" and the end slot never break
  // inside themselves; where a box is narrower still, the row wraps rather than
  // overflows.
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

  // The media query of the `max-[1099px]:` classes below, asked when a tooltip would open.
  const labelsHidden = () =>
    typeof matchMedia === 'function' && matchMedia('(max-width: 1099px)').matches;
</script>

<!-- A step: its tooltip sits on a wrapper, since a disabled button fires no pointer events
     (DESIGN.md §5), and shows only while the label is hidden. `p-2` makes the icon-only button
     the square of a labelled one's height. -->
{#snippet step(
  testid: string,
  label: string,
  icon: IconName,
  iconAfter: boolean,
  off: boolean,
  target: number,
)}
  <span
    class="inline-flex"
    use:tooltip={{ text: label, describe: false, whenOverflowing: true, alsoClipped: labelsHidden }}
  >
    <button
      type="button"
      class="btn-secondary btn-sm inline-flex items-center gap-1 max-[1099px]:p-2"
      data-testid={testid}
      disabled={off}
      onclick={() => onPage(target)}
    >
      {#if !iconAfter}<Icon name={icon} size={16} />{/if}
      <span class="max-[1099px]:sr-only">{label}</span>
      {#if iconAfter}<Icon name={icon} size={16} />{/if}
    </button>
  </span>
{/snippet}

<div class="flex flex-wrap items-center gap-2 text-sm text-secondary pt-2">
  <span class="flex-1"></span>
  {@render step('pg-first', $t('pagination.first'), 'chevronFirst', false, atFirst, 0)}
  {@render step('pg-prev', $t('pagination.prev'), 'chevronLeft', false, atFirst, page - 1)}
  <span class="whitespace-nowrap" data-testid="pg-label">
    {$t('pagination.pageOf', { page: page + 1, total: pageCount })}
  </span>
  {@render step('pg-next', $t('pagination.next'), 'chevronRight', true, atLast, page + 1)}
  {@render step('pg-last', $t('pagination.last'), 'chevronLast', true, atLast, lastIndex)}
  <span class="flex-1 flex justify-end">
    {@render end?.()}
  </span>
</div>

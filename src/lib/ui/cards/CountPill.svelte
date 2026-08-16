<script module lang="ts">
  // The rounded-full numeric badge: "3 modpack updates", "2 running". Extracted
  // because three surfaces had hand-rolled the same recipe and the copies had
  // already drifted on box size (15px in the compact chrome, 18px inline) and
  // on `leading-none`.
  //
  // Same split as ToggleChip: the component for the ordinary <span> case, plus
  // an exported class builder for the two running-count popovers, whose pill IS
  // their <button> trigger and so cannot nest an element inside a button.
  //
  // Size is a named token rather than a free number, mirroring CardMedia's
  // sm/md/lg: the two that exist are real densities (the compact ModeSwitcher
  // rail vs an inline label row), so the scale is owned here instead of being
  // re-guessed per call site.
  //
  // There is no tone: a COUNT is always the success tone. A pill carrying a
  // WORD is a status, and belongs in StatusBadge (§9) — see the
  // ManageInstancesModal "Active" marker, migrated in the same pass.
  export type CountPillSize = 'sm' | 'md';

  const BASE =
    'inline-flex items-center justify-center rounded-full bg-success px-1 text-[10px] font-semibold leading-none text-white';
  const SIZE: Record<CountPillSize, string> = {
    sm: 'h-[15px] min-w-[15px]',
    md: 'h-[18px] min-w-[18px]',
  };

  export function countPillClass(size: CountPillSize): string {
    return `${BASE} ${SIZE[size]}`;
  }
</script>

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { tooltip } from '$lib/ui/tooltip';

  let {
    size = 'md',
    title = undefined,
    testid = undefined,
    class: klass = '',
    children,
  }: {
    size?: CountPillSize;
    /**
     * Hover/focus label, routed through the shared tooltip layer — never a
     * native `title=` (docs/DESIGN.md §5). Prop name matches StatusBadge.
     */
    title?: string | undefined;
    testid?: string | undefined;
    /** Positioning only (e.g. `ml-1`). The pill recipe is not overridable. */
    class?: string;
    children: Snippet;
  } = $props();
</script>

<span class={`${countPillClass(size)} ${klass}`} use:tooltip={title} data-testid={testid}>
  {@render children()}
</span>

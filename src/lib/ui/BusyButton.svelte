<script lang="ts">
  // Shared async-action button. Controlled: the parent owns `busy` and sets it
  // around the IPC call (before the first await, cleared in a finally). When
  // busy, the button is disabled and a small Spinner renders alongside the
  // label — the label stays visible. Arbitrary extra attributes (data-testid,
  // data-tour, aria-label, ...) are forwarded onto the <button>.
  import type { Snippet } from 'svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { tooltip } from '$lib/ui/tooltip';

  interface Props {
    busy?: boolean;
    disabled?: boolean;
    type?: 'button' | 'submit';
    class?: string;
    spinnerClass?: string;
    /**
     * Hover/focus label, routed through the shared tooltip layer — never a
     * native `title=` (docs/DESIGN.md §5). Kept as a named prop rather than
     * deleted precisely BECAUSE of `...rest` below: destructuring it here is
     * what stops a caller's `title="…"` from falling through onto the DOM,
     * where the source guard (tests/no-native-title.test.ts) could not see it.
     * Prop name matches StatusBadge; §5 blesses it — "A `title` prop forwarded
     * *into* `use:tooltip` internally is fine — the prop name is incidental."
     * For a DISABLED BusyButton the reason still belongs on a wrapping <span>
     * (§11): the button element itself fires no pointer events.
     */
    title?: string;
    onclick?: () => void;
    children: Snippet;
    [key: string]: unknown;
  }
  let {
    busy = false,
    disabled = false,
    type = 'button',
    class: klass = '',
    spinnerClass = '',
    title,
    onclick,
    children,
    ...rest
  }: Props = $props();
</script>

<button
  {type}
  class={klass}
  disabled={busy || disabled}
  aria-busy={busy ? 'true' : 'false'}
  use:tooltip={title}
  {onclick}
  {...rest}
>
  <span class="inline-flex items-center justify-center gap-2">
    {#if busy}
      <Spinner size="sm" class={spinnerClass} />
    {/if}
    {@render children()}
  </span>
</button>

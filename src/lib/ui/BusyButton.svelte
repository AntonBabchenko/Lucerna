<script lang="ts">
  // Shared async-action button. Controlled: the parent owns `busy` and sets it
  // around the IPC call (before the first await, cleared in a finally). When
  // busy, the button is disabled and a small Spinner renders alongside the
  // label — the label stays visible. Arbitrary extra attributes (data-testid,
  // data-tour, aria-label, ...) are forwarded onto the <button>.
  import type { Snippet } from 'svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  interface Props {
    busy?: boolean;
    disabled?: boolean;
    type?: 'button' | 'submit';
    class?: string;
    spinnerClass?: string;
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
  {title}
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

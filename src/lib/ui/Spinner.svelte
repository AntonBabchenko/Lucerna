<script lang="ts">
  // Shared loading spinner. Inherits `currentColor` for the active arc, so
  // place it inside a `text-*` context to colour it (e.g. text-secondary).
  //
  // `delayMs` implements the standard anti-flicker pattern: the spinner is
  // not rendered until the load has run longer than `delayMs`, so fast
  // responses never flash a spinner. Callers just render `{#if loading}` —
  // when the load finishes before the delay, the component unmounts and the
  // pending timeout is cleared, so nothing ever appears.
  import { t } from '$lib/i18n';

  interface Props {
    size?: 'sm' | 'md' | 'lg';
    delayMs?: number;
    label?: string;
    class?: string;
  }
  let { size = 'md', delayMs = 0, label, class: klass = '' }: Props = $props();

  const resolvedLabel = $derived(label ?? $t('common.loading'));

  const SIZES: Record<NonNullable<Props['size']>, string> = {
    sm: 'h-4 w-4 border-2',
    md: 'h-6 w-6 border-2',
    lg: 'h-8 w-8 border-[3px]',
  };

  // `elapsed` flips true once the delay timer fires. `visible` is derived so
  // that with no delay it is true on the very first render (synchronous, no
  // post-mount flash) and with a delay it follows the timer.
  let elapsed = $state(false);
  $effect(() => {
    if (delayMs <= 0) return;
    elapsed = false;
    const t = setTimeout(() => (elapsed = true), delayMs);
    return () => clearTimeout(t);
  });
  const visible = $derived(delayMs <= 0 || elapsed);
</script>

{#if visible}
  <span
    role="status"
    aria-label={resolvedLabel}
    class="inline-flex items-center justify-center {klass}"
  >
    <span
      class="inline-block animate-spin rounded-full border-current border-r-transparent {SIZES[
        size
      ]}"
      aria-hidden="true"
    ></span>
    <span class="sr-only">{resolvedLabel}</span>
  </span>
{/if}

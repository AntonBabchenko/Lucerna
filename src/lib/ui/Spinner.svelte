<script lang="ts">
  // Shared loading spinner. Inherits `currentColor` for the active arc, so
  // place it inside a `text-*` context to colour it (e.g. text-secondary).
  //
  // `delayMs` implements the standard anti-flicker pattern: the spinner is
  // not rendered until the load has run longer than `delayMs`.
  //
  // `labelPlacement` controls the label:
  //   'sr-only' (default) — circle only; label is screen-reader-only (legacy).
  //   'right'             — circle + visible label to the right (inline/buttons).
  //   'below'             — circle + visible label centered underneath (panels).
  // The wrapper always carries one role="status" + aria-label, and any VISIBLE
  // label is aria-hidden, so assistive tech announces the state exactly once.
  import { t } from '$lib/i18n';

  interface Props {
    size?: 'sm' | 'md' | 'lg';
    delayMs?: number;
    label?: string;
    labelPlacement?: 'sr-only' | 'right' | 'below';
    class?: string;
  }
  let {
    size = 'md',
    delayMs = 0,
    label,
    labelPlacement = 'sr-only',
    class: klass = '',
  }: Props = $props();

  const resolvedLabel = $derived(label ?? $t('common.loading'));

  const SIZES: Record<NonNullable<Props['size']>, string> = {
    sm: 'h-4 w-4 border-2',
    md: 'h-6 w-6 border-2',
    lg: 'h-8 w-8 border-[3px]',
  };

  const WRAPPER: Record<NonNullable<Props['labelPlacement']>, string> = {
    'sr-only': 'inline-flex items-center justify-center',
    right: 'inline-flex items-center gap-2',
    below: 'inline-flex flex-col items-center justify-center gap-2',
  };

  let elapsed = $state(false);
  $effect(() => {
    if (delayMs <= 0) return;
    elapsed = false;
    const timer = setTimeout(() => (elapsed = true), delayMs);
    return () => clearTimeout(timer);
  });
  const visible = $derived(delayMs <= 0 || elapsed);
</script>

{#if visible}
  <span role="status" aria-label={resolvedLabel} class="{WRAPPER[labelPlacement]} {klass}"><span
      class="inline-block animate-spin rounded-full border-current border-r-transparent {SIZES[
        size
      ]}"
      aria-hidden="true"
    ></span>{#if labelPlacement === 'sr-only'}<span class="sr-only">{resolvedLabel}</span>{:else}<span class="text-sm" aria-hidden="true">{resolvedLabel}</span>{/if}</span>
{/if}

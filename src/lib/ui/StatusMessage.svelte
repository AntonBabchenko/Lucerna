<script lang="ts">
  import { Icon } from '$lib/ui/icons';

  let {
    message,
    tone = 'danger',
    live,
    withIcon = false,
    reserveSpace = false,
    class: className = '',
  }: {
    /**
     * The message to announce. When null the live region stays present but
     * empty, so a later null→text transition is announced (aria-atomic).
     */
    message: string | null;
    tone?: 'danger' | 'warning' | 'info';
    /** Defaults to assertive for danger, polite for advisory tones. */
    live?: 'assertive' | 'polite';
    /** Render a leading warning icon before the text. */
    withIcon?: boolean;
    /** Reserve one line of height even when empty, to avoid layout shift. */
    reserveSpace?: boolean;
    /** Extra classes applied to the visible message element (e.g. box styling). */
    class?: string;
  } = $props();

  const effectiveLive = $derived(live ?? (tone === 'danger' ? 'assertive' : 'polite'));
  const toneClass = $derived(
    tone === 'danger' ? 'text-danger' : tone === 'warning' ? 'text-warning-text' : 'text-secondary',
  );
</script>

<!--
  Persistent live region: the OUTER element is always rendered so screen readers
  announce when text appears (role=alert/status + aria-atomic). The visible
  styled message is the INNER element, rendered only when there is a message, so
  callers can pass box styling via `class` without an empty box in the idle state.
-->
<div
  role={effectiveLive === 'assertive' ? 'alert' : 'status'}
  aria-atomic="true"
  class={reserveSpace ? 'min-h-4' : undefined}
>
  {#if message}
    <p class="text-xs {toneClass} {className}">
      {#if withIcon}
        <span class="flex items-center gap-1.5"><Icon name="warning" /> {message}</span>
      {:else}
        {message}
      {/if}
    </p>
  {/if}
</div>

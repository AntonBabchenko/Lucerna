<script lang="ts">
  import type { ReportLine } from '$lib/format/reason-lines';
  import { Icon } from '$lib/ui/icons';

  let {
    message,
    details = [],
    tone = 'danger',
    live,
    withIcon = false,
    reserveSpace = false,
    class: className = '',
    dataTestid,
    id,
    element = $bindable(null),
  }: {
    /**
     * The message to announce. When null the live region stays present but
     * empty, so a later null→text transition is announced (aria-atomic).
     */
    message: string | null;
    /**
     * Lines that explain the message (why a repair left something behind),
     * listed under it inside the same region — one announcement. Shown only
     * with a message. A reason with the names it stopped (`reasonLines`) puts
     * them on a line of their own, above it.
     */
    details?: readonly ReportLine[];
    /** danger → role=alert; warning / info / success → role=status. success is the "set" / ok tone (DESIGN §10). */
    tone?: 'danger' | 'warning' | 'info' | 'success';
    /** Defaults to assertive for danger, polite for advisory tones. */
    live?: 'assertive' | 'polite';
    /** Render a leading warning icon before the text. */
    withIcon?: boolean;
    /** Reserve one line of height even when empty, to avoid layout shift. */
    reserveSpace?: boolean;
    /** Extra classes applied to the visible message element (e.g. box styling). */
    class?: string;
    /** Test hook on the live region itself — never wrap the component for one. */
    dataTestid?: string;
    /** The region's id, for a field that names it in `aria-describedby` while it shows. */
    id?: string;
    /** The live region element, for a caller that scrolls it into view. */
    element?: HTMLElement | null;
  } = $props();

  const effectiveLive = $derived(live ?? (tone === 'danger' ? 'assertive' : 'polite'));
  const toneClass = $derived(
    tone === 'danger'
      ? 'text-danger'
      : tone === 'warning'
        ? 'text-warning-text'
        : tone === 'success'
          ? 'text-success'
          : 'text-secondary',
  );
</script>

<!--
  Persistent live region: the OUTER element is always rendered so screen readers
  announce when text appears (role=alert/status + aria-atomic). The visible
  styled message is the INNER element, rendered only when there is a message, so
  callers can pass box styling via `class` without an empty box in the idle state.
  While idle the region is `absolute`: out of flow, so it takes no flex/grid gap
  slot, but still in the accessibility tree — never hidden, which would stop the
  next message from being announced. The same node goes back into flow when a
  message arrives. `reserveSpace` keeps the idle line in flow on purpose. A test
  hook goes on the region (`dataTestid`): a wrapper would be an empty box again.
-->
<div
  bind:this={element}
  {id}
  role={effectiveLive === 'assertive' ? 'alert' : 'status'}
  aria-atomic="true"
  data-testid={dataTestid}
  class={reserveSpace ? 'min-h-4' : message ? undefined : 'absolute'}
>
  {#if message}
    <p class="text-xs {toneClass} {className}">
      {#if withIcon}
        <span class="flex items-center gap-1.5"><Icon name="warning" /> {message}</span>
      {:else}
        {message}
      {/if}
    </p>
    {#if details.length > 0}
      <ul class="mt-1 list-disc pl-5 text-xs {toneClass}">
        {#each details as line}
          <!-- The names a reason stopped, then the reason under them: never after a colon. -->
          <li>
            {#if typeof line === 'string'}
              {line}
            {:else}
              <span class="block">{line.names}</span>
              <span class="block">{line.reason}</span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</div>

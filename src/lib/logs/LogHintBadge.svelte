<script lang="ts">
  // The per-line hint badge shared by both log views (standard + structured
  // crash). Absolutely positioned inside the row's reserved gutter (the row
  // gets `relative pl-6` when the file has any annotations), so it never
  // shifts the log text out of column alignment. Pointer hover is handled by
  // the ROW (hover bridge); this button is the keyboard path — focus-visible
  // opens the card instantly, blur arms the grace close.
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';

  let {
    onActivate,
    onLeave,
  }: { onActivate: (el: HTMLElement, viaFocus: boolean) => void; onLeave: () => void } = $props();
</script>

<button
  type="button"
  class="log-hint-badge text-accent"
  aria-label={$t('logs.hints.badgeAriaLabel')}
  onfocus={(e) => {
    if ((e.currentTarget as HTMLElement).matches(':focus-visible')) {
      onActivate(e.currentTarget as HTMLElement, true);
    }
  }}
  onblur={() => onLeave()}
>
  <Icon name="info" size={12} />
</button>

<style>
  /* Absolute inside the row's gutter (row is `relative` when the gutter is
     on), out of the text flow entirely. */
  .log-hint-badge {
    position: absolute;
    left: 0.3rem;
    top: 0.15em;
    display: inline-flex;
    opacity: 0.85;
  }

  .log-hint-badge:hover,
  .log-hint-badge:focus-visible {
    opacity: 1;
  }
</style>

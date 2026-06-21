<script lang="ts">
  import { Icon, type IconName } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';

  // The leading icon of a sidebar nav button. `iconClass` carries the state
  // colour/pulse from navVisual(). When the icon itself conveys status (running,
  // crashed, advisory) `statusLabel` is set: the icon is wrapped in a
  // role="img" + tooltip span so the state is never colour-only (DESIGN.md §13).
  // For wrench states the label is null here — the wrench carries the name.
  let {
    name,
    size = 16,
    iconClass = '',
    statusLabel = null,
  }: {
    name: IconName;
    size?: number;
    iconClass?: string;
    statusLabel?: string | null;
  } = $props();
</script>

{#if statusLabel}
  <span class="inline-flex" role="img" aria-label={statusLabel} use:tooltip={statusLabel}>
    <Icon {name} {size} class={iconClass} />
  </span>
{:else}
  <Icon {name} {size} class={iconClass} />
{/if}

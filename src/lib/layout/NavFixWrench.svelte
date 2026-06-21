<script lang="ts">
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';

  // The inline "a one-click fix is available" wrench, rendered after a nav
  // button's label. Carries its own accessible name + tooltip.
  let { label, testid }: { label: string; testid?: string } = $props();
</script>

<span
  class="text-warning-text inline-flex"
  role="img"
  aria-label={label}
  use:tooltip={label}
  data-testid={testid}
>
  <Icon name="wrench" size={12} />
</span>

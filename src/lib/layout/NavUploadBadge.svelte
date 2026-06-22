<script lang="ts">
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';

  // The inline "an upload to hosting is in progress" badge, rendered after the
  // Servers nav button's label. Coexists with the icon's running/crashed status
  // (upload and local-running are orthogonal). Mirrors NavFixWrench.
  let { label, testid }: { label: string; testid?: string } = $props();
</script>

<span
  class="text-accent inline-flex animate-spin motion-reduce:animate-none"
  role="img"
  aria-label={label}
  use:tooltip={label}
  data-testid={testid}
>
  <Icon name="refresh" size={12} />
</span>

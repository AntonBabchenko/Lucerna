<script lang="ts">
  import Spinner from '$lib/ui/Spinner.svelte';
  import { tooltip } from '$lib/ui/tooltip';

  // The inline "an upload to hosting is in progress" badge, rendered after the
  // Servers nav button's label. Coexists with the icon's running/crashed status
  // (upload and local-running are orthogonal). Mirrors NavFixWrench.
  //
  // Continuous activity spin renders through the shared <Spinner> (DESIGN.md §5
  // — no ad-hoc animate-spin); Spinner owns the role="status" + aria-label.
  let { label, testid }: { label: string; testid?: string } = $props();
</script>

<span class="text-accent inline-flex" use:tooltip={label} data-testid={testid}>
  <Spinner size="sm" {label} />
</span>

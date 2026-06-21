<script lang="ts">
  // The restore affordance for a dismissed diagnosis/attention banner: a small
  // amber warning-triangle pill shown near a surface's status/header. Mirrors the
  // overview attention-restore badge in InstanceHeader (same amber-pill language)
  // so the "bring the hidden warning back" control looks the same everywhere.
  // See docs/DESIGN.md §10.
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';

  let { onRestore, testid = undefined }: { onRestore: () => void; testid?: string } = $props();
</script>

<button
  type="button"
  class="inline-flex items-center rounded-full border border-warning-text bg-warning-bg px-2.5 py-1.5
    text-warning-text transition-colors hover:bg-warning-text/10
    focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2"
  aria-label={$t('common.restoreWarning')}
  use:tooltip={$t('common.restoreWarning')}
  onclick={onRestore}
  data-testid={testid}
>
  <Icon name="warning" size={16} />
</button>

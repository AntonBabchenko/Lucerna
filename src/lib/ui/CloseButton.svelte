<script lang="ts">
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';

  let {
    onClick,
    ariaLabel,
    class: extraClass = '',
  }: {
    onClick?: () => void;
    ariaLabel?: string;
    class?: string;
  } = $props();

  const resolvedLabel = $derived(ariaLabel ?? $t('common.close'));
</script>

<button
  type="button"
  class={`btn-icon ${extraClass}`}
  aria-label={resolvedLabel}
  use:tooltip={resolvedLabel}
  onclick={() => onClick?.()}><Icon name="close" /></button
>

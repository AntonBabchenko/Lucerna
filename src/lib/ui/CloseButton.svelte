<script lang="ts">
  import { t } from '$lib/i18n';

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
  onclick={() => onClick?.()}>×</button
>

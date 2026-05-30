<script lang="ts">
  let {
    onClick,
    ariaLabel = 'Close',
    class: extraClass = '',
  }: {
    onClick?: () => void;
    ariaLabel?: string;
    class?: string;
  } = $props();
</script>

<button
  type="button"
  class={`btn-icon ${extraClass}`}
  aria-label={ariaLabel}
  onclick={() => onClick?.()}>×</button
>

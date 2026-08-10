<script lang="ts">
  // Single-select chip group (radiogroup). Renders one role="radio" button per
  // option with aria-checked + roving arrow-key navigation, styled identically
  // to ToggleChip via the shared toggleChipClass() helper. For multi-select use
  // bare ToggleChip[] instead.
  import { Icon, type IconName } from '$lib/ui/icons';
  import { toggleChipClass, type ToggleChipTone } from '$lib/ui/ToggleChip.svelte';
  import { nextRovingIndex } from '$lib/ui/roving';

  type Option = {
    value: string;
    label: string;
    tone: ToggleChipTone;
    icon?: IconName;
    count?: number;
    testId?: string;
  };
  let {
    options,
    value,
    onChange,
    ariaLabel,
  }: {
    options: Option[];
    value: string;
    onChange: (v: string) => void;
    ariaLabel: string;
  } = $props();

  let btnEls = $state<(HTMLButtonElement | null)[]>([]);

  // Roving focus: Left/Right (and Up/Down) wrap, Home/End jump.
  function onKeydown(e: KeyboardEvent) {
    const current = options.findIndex((o) => o.value === value);
    const next = nextRovingIndex(e.key, current, options.length, 'both');
    if (next === null) return;
    e.preventDefault();
    const target = options[next];
    if (!target) return;
    onChange(target.value);
    btnEls[next]?.focus();
  }
</script>

<!-- svelte-ignore a11y_interactive_supports_focus -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  role="radiogroup"
  aria-label={ariaLabel}
  onkeydown={onKeydown}
  class="inline-flex flex-wrap items-center gap-2"
>
  {#each options as option, i (option.value)}
    {@const active = value === option.value}
    <button
      bind:this={btnEls[i]}
      type="button"
      role="radio"
      aria-checked={active}
      tabindex={active || (i === 0 && !options.some((o) => o.value === value)) ? 0 : -1}
      data-testid={option.testId}
      class={toggleChipClass(active, option.tone)}
      onclick={() => onChange(option.value)}
    >
      {#if option.icon}<Icon name={option.icon} size={14} />{/if}
      <span>{option.label}</span>
      {#if option.count !== undefined}<span class="opacity-70">{option.count}</span>{/if}
    </button>
  {/each}
</div>

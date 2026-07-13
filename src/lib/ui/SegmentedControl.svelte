<script lang="ts">
  // A small two-variant segment control used by the browse-layout toggle
  // (variant="boxed") and the page-size picker (variant="inline"). Each option
  // is a <button> with aria-pressed; roving arrow-key focus mirrors TabBar.
  // An option's `label` is rendered as visible text only when it has no `icon`;
  // icon-only options use `label` (falling back to the group ariaLabel) as their
  // accessible name + tooltip, so they stay compact but remain labelled.
  import { Icon, type IconName } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { nextRovingIndex } from '$lib/ui/roving';

  type Option = { value: string; label?: string; icon?: IconName; testId?: string };
  let {
    options,
    value,
    onChange,
    variant,
    ariaLabel,
  }: {
    options: Option[];
    value: string;
    onChange: (v: string) => void;
    variant: 'boxed' | 'inline';
    ariaLabel: string;
  } = $props();

  // DOM-ordered button refs so arrow keys can move focus to a sibling.
  let btnEls = $state<(HTMLButtonElement | null)[]>([]);

  // Roving keyboard support (mirrors TabBar): Left/Right wrap, Home/End jump.
  // Activation follows focus — selecting on move is expected for a segmented
  // control whose options act immediately.
  function onKeydown(e: KeyboardEvent) {
    const current = options.findIndex((o) => o.value === value);
    const next = nextRovingIndex(e.key, current, options.length, 'horizontal');
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
  role="group"
  aria-label={ariaLabel}
  onkeydown={onKeydown}
  class={variant === 'boxed'
    ? 'inline-flex border border-border-subtle rounded overflow-hidden'
    : 'inline-flex items-center gap-2 text-sm'}
>
  {#each options as option, i (option.value)}
    {@const active = value === option.value}
    <button
      bind:this={btnEls[i]}
      type="button"
      aria-pressed={active}
      aria-label={option.icon ? (option.label ?? ariaLabel) : undefined}
      tabindex={active ? 0 : -1}
      data-testid={option.testId}
      class={variant === 'boxed'
        ? // Swap btn-primary/btn-ghost conditionally: two btn-* purpose classes
          // must never be stacked on one element — the later app.css rule
          // (.btn-secondary) wins the equal-specificity cascade and kills the
          // active fill.
          `${active ? 'btn-primary' : 'btn-ghost'} btn-sm rounded-none`
        : `px-0.5 ${active ? 'text-primary font-semibold' : 'text-secondary hover:text-primary'}`}
      use:tooltip={option.icon ? (option.label ?? ariaLabel) : null}
      onclick={() => onChange(option.value)}
    >
      {#if option.icon}<Icon name={option.icon} />{/if}{#if !option.icon}{option.label ?? ''}{/if}
    </button>
  {/each}
</div>

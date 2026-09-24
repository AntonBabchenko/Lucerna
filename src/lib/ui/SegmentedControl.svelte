<script lang="ts">
  // A small two-variant segment control used by the browse-layout toggle and the
  // screenshot toggles (variant="boxed") and the page-size picker
  // (variant="inline"). Each option is a <button> with aria-pressed; roving
  // arrow-key focus mirrors TabBar. Boxed: a recessed track whose chosen segment
  // is a neutral raised thumb with a small accent mark — a chosen option is a
  // state, so no btn-* purpose class (a CTA fill read as "press me").
  // An option's `label` is rendered as visible text only when it has no `icon`;
  // icon-only options use `label` (falling back to the group ariaLabel) as their
  // accessible name + tooltip, so they stay compact but remain labelled.
  // Settings uses the boxed variant for the theme, tip-level and game-start pickers, naming
  // the group through ariaLabel and linking its hint through describedby.
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
    dataTestid,
    describedby,
    disabled = false,
  }: {
    options: Option[];
    /** null = no value to show (a setting still loading or unreadable). */
    value: string | null;
    onChange: (v: string) => void;
    variant: 'boxed' | 'inline';
    ariaLabel: string;
    /** `data-testid` on the group element. */
    dataTestid?: string;
    /** Forwarded as `aria-describedby` on the group (a hint line under the control). */
    describedby?: string;
    /** Every option disabled and arrows ignored (pending / failed settings). */
    disabled?: boolean;
  } = $props();
  // With no value (a setting still loading or unreadable) nothing is pressed, yet
  // the group keeps ONE tab stop — its first option — and the arrow keys start
  // from there. Disabled: every option disabled and the arrows ignored.
  const activeIndex = $derived(options.findIndex((o) => o.value === value));
  const tabStop = $derived(activeIndex < 0 ? 0 : activeIndex);

  // DOM-ordered button refs so arrow keys can move focus to a sibling.
  let btnEls = $state<(HTMLButtonElement | null)[]>([]);

  // Roving keyboard support (mirrors TabBar): Left/Right wrap, Home/End jump.
  // Activation follows focus — selecting on move is expected for a segmented
  // control whose options act immediately.
  function onKeydown(e: KeyboardEvent) {
    if (disabled) return;
    const next = nextRovingIndex(e.key, tabStop, options.length, 'horizontal');
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
  aria-describedby={describedby}
  data-testid={dataTestid}
  onkeydown={onKeydown}
  class={variant === 'boxed'
    ? // w-fit: in a flex column an inline-flex root is stretched to the column's
      // width. No overflow-hidden: it clipped the focus ring to a sliver.
      'inline-flex w-fit max-w-full gap-0.5 rounded-md bg-control-track p-0.5'
    : 'inline-flex items-center gap-2 text-sm'}
>
  {#each options as option, i (option.value)}
    {@const active = value === option.value}
    <button
      bind:this={btnEls[i]}
      type="button"
      aria-pressed={active}
      aria-label={option.icon ? (option.label ?? ariaLabel) : undefined}
      tabindex={i === tabStop ? 0 : -1}
      {disabled}
      data-testid={option.testId}
      class={variant === 'boxed'
        ? // One weight for every segment, so a new choice never shifts widths. The
          // hover tint is half a thumb: it can never look chosen. focus-visible:z-10
          // lifts the global focus ring over the neighbouring thumb.
          `relative inline-flex h-7 items-center justify-center rounded border pb-0.5 text-sm font-medium transition-colors focus-visible:z-10 disabled:cursor-not-allowed disabled:opacity-50 ${option.icon ? 'px-2' : 'px-3'} ${active ? 'cursor-default border-border-emphasis bg-control-thumb text-primary' : 'border-transparent text-secondary enabled:hover:bg-control-thumb/50 enabled:hover:text-primary'}`
        : `px-0.5 disabled:opacity-50 disabled:cursor-not-allowed ${active ? 'text-primary font-semibold' : 'text-secondary hover:text-primary'}`}
      use:tooltip={option.icon ? (option.label ?? ariaLabel) : null}
      onclick={() => onChange(option.value)}
    >
      {#if option.icon}<Icon name={option.icon} />{/if}{#if !option.icon}{option.label ?? ''}{/if}
      {#if variant === 'boxed' && active}
        <!-- The chosen mark: the accent TEXT tier (bg-current + text-accent), which
             clears 3:1 on the thumb in both themes; plain --accent does not in dark. -->
        <span
          data-segment-mark
          aria-hidden="true"
          class="pointer-events-none absolute bottom-0 left-1/2 h-[3px] w-4 -translate-x-1/2 rounded-full bg-current text-accent"
        ></span>
      {/if}
    </button>
  {/each}
</div>

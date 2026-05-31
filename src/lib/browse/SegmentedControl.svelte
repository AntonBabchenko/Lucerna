<script lang="ts">
  // Accessible single-select segmented control. role=radiogroup with
  // roving tabindex + arrow-key selection, mirroring the keyboard
  // pattern InstalledModsView's filter radiogroup already establishes.
  // Callback API (value + onChange) rather than bind:value so callers
  // can keep their own narrowly-typed union state without a string-vs-
  // union binding mismatch.
  type Option = { value: string; label: string };

  let {
    value,
    options,
    ariaLabel,
    testid,
    onChange,
  }: {
    value: string;
    options: Option[];
    ariaLabel: string;
    testid?: string;
    onChange: (value: string) => void;
  } = $props();

  let groupEl: HTMLDivElement | undefined = $state();

  // Index of the currently-selected option; falls back to 0 so the
  // group always has exactly one tabbable radio even if `value` matches
  // nothing in `options`.
  const selectedIndex = $derived(
    Math.max(
      0,
      options.findIndex((o) => o.value === value),
    ),
  );

  function focusRadio(i: number) {
    const radios = groupEl?.querySelectorAll<HTMLButtonElement>('[role="radio"]');
    radios?.[i]?.focus();
  }

  function select(i: number) {
    onChange(options[i]!.value);
    focusRadio(i);
  }

  function onKeyDown(e: KeyboardEvent) {
    const last = options.length - 1;
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
      e.preventDefault();
      select(Math.min(selectedIndex + 1, last));
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
      e.preventDefault();
      select(Math.max(selectedIndex - 1, 0));
    } else if (e.key === 'Home') {
      e.preventDefault();
      select(0);
    } else if (e.key === 'End') {
      e.preventDefault();
      select(last);
    }
  }
</script>

<div
  bind:this={groupEl}
  role="radiogroup"
  aria-label={ariaLabel}
  data-testid={testid}
  tabindex={-1}
  class="inline-flex w-full rounded border border-border-emphasis overflow-hidden"
  onkeydown={onKeyDown}
>
  {#each options as opt, i (opt.value)}
    <button
      type="button"
      role="radio"
      aria-checked={value === opt.value}
      tabindex={i === selectedIndex ? 0 : -1}
      class={`flex-1 px-2 py-1 text-sm text-center border-r border-border-subtle last:border-r-0 ${value === opt.value ? 'bg-accent/15' : ''}`}
      class:text-accent={value === opt.value}
      class:font-medium={value === opt.value}
      class:text-secondary={value !== opt.value}
      onclick={() => select(i)}
    >
      {opt.label}
    </button>
  {/each}
</div>

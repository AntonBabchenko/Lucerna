<script lang="ts">
  // Fully-themeable select-only listbox. Replaces native <select> across
  // the app: WebKitGTK (Linux) draws the native popup as an OS widget that
  // ignores CSS, so the open option list stayed light even in dark theme.
  // A custom button + role=listbox popover follows the theme tokens on
  // every platform.
  //
  // The popover is position:fixed (measured from the trigger each open,
  // clamped to the viewport, flips above when short on room below) so it
  // escapes the overflow box of the sidebar / modals — mirrors
  // InstanceConceptTooltip.svelte.
  //
  // Keyboard model is the WAI-ARIA "select-only combobox": focus stays on
  // the trigger button and the active option is tracked via
  // aria-activedescendant (never moves DOM focus into the list).

  type Primitive = string | number;
  type Option = { value: Primitive; label: string; disabled?: boolean };

  let {
    value,
    options,
    onChange,
    placeholder,
    disabled = false,
    ariaLabel,
    id,
    class: className = '',
    dataTestid,
  }: {
    value: Primitive | null;
    options: Option[];
    onChange: (value: Primitive) => void;
    placeholder?: string;
    disabled?: boolean;
    ariaLabel?: string;
    id?: string;
    class?: string;
    dataTestid?: string;
  } = $props();

  const MAX_POPOVER_HEIGHT = 240; // keep in sync with max-h-60 on the list
  const GAP = 4;
  const MARGIN = 8;

  let open = $state(false);
  let activeIndex = $state(-1);
  let trigger: HTMLButtonElement | undefined = $state();
  let listEl: HTMLUListElement | undefined = $state();
  let popoverTop = $state(0);
  let popoverLeft = $state(0);
  let popoverWidth = $state(0);
  let flipUp = $state(false);

  // Unique ids so multiple Selects can coexist (filter rows stack several).
  const uid = crypto.randomUUID();
  const listboxId = `select-list-${uid}`;
  const optionId = (i: number) => `select-opt-${uid}-${i}`;

  const selectedIndex = $derived(options.findIndex((o) => o.value === value));
  const selectedLabel = $derived(selectedIndex >= 0 ? options[selectedIndex].label : null);
  const displayLabel = $derived(selectedLabel ?? placeholder ?? '');
  // Grey the trigger when nothing meaningful is chosen: no matching option,
  // or an explicit empty value (the "Any…" / "Choose…" rows filters keep).
  const isPlaceholder = $derived(selectedLabel === null || value === '' || value === null);
  const activeDescendant = $derived(open && activeIndex >= 0 ? optionId(activeIndex) : undefined);
  const popoverStyle = $derived(
    flipUp
      ? `bottom: ${window.innerHeight - popoverTop}px; left: ${popoverLeft}px; min-width: ${popoverWidth}px;`
      : `top: ${popoverTop}px; left: ${popoverLeft}px; min-width: ${popoverWidth}px;`,
  );

  function positionPopover() {
    if (!trigger) return;
    const r = trigger.getBoundingClientRect();
    popoverWidth = r.width;
    const maxLeft = window.innerWidth - r.width - MARGIN;
    popoverLeft = Math.min(Math.max(r.left, MARGIN), Math.max(MARGIN, maxLeft));
    const below = window.innerHeight - r.bottom;
    flipUp = below < MAX_POPOVER_HEIGHT + GAP && r.top > below;
    popoverTop = flipUp ? r.top - GAP : r.bottom + GAP;
  }

  // Walk from `start` in direction `dir` to the first non-disabled option.
  function firstEnabledFrom(start: number, dir: 1 | -1): number {
    let i = start;
    while (i >= 0 && i < options.length) {
      if (!options[i].disabled) return i;
      i += dir;
    }
    return -1;
  }

  function openList() {
    if (disabled) return;
    positionPopover();
    open = true;
    activeIndex =
      selectedIndex >= 0 && !options[selectedIndex].disabled
        ? selectedIndex
        : firstEnabledFrom(0, 1);
  }

  function closeList() {
    open = false;
    activeIndex = -1;
    trigger?.focus();
  }

  function commit(i: number) {
    if (i < 0 || i >= options.length || options[i].disabled) return;
    onChange(options[i].value);
    open = false;
    activeIndex = -1;
    trigger?.focus();
  }

  // First-letter typeahead: accumulate typed chars within a short window and
  // jump to the first option whose label starts with the buffer. Open → move
  // active; closed → commit directly (native <select> parity).
  let typeBuffer = '';
  let typeTimer: ReturnType<typeof setTimeout> | undefined;
  function typeahead(ch: string) {
    typeBuffer += ch.toLowerCase();
    clearTimeout(typeTimer);
    typeTimer = setTimeout(() => (typeBuffer = ''), 500);
    const match = options.findIndex(
      (o) => !o.disabled && o.label.toLowerCase().startsWith(typeBuffer),
    );
    if (match < 0) return;
    if (open) activeIndex = match;
    else commit(match);
  }

  function isTypeaheadKey(e: KeyboardEvent): boolean {
    return e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey;
  }

  function onKeyDown(e: KeyboardEvent) {
    if (disabled) return;
    if (!open) {
      if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        openList();
      } else if (isTypeaheadKey(e)) {
        e.preventDefault();
        typeahead(e.key);
      }
      return;
    }
    // Keys consumed while open are stopped from propagating, so an enclosing
    // modal's `<svelte:window onkeydown>` (Escape-to-close, etc.) doesn't also
    // fire — pressing Escape should dismiss only the dropdown, matching native
    // <select>. Tab is the exception: it commits but must keep bubbling/default
    // so focus moves on to the next control.
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      e.stopPropagation();
      const next = firstEnabledFrom(activeIndex + 1, 1);
      if (next >= 0) activeIndex = next;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      e.stopPropagation();
      const prev = firstEnabledFrom(activeIndex - 1, -1);
      if (prev >= 0) activeIndex = prev;
    } else if (e.key === 'Home') {
      e.preventDefault();
      e.stopPropagation();
      activeIndex = firstEnabledFrom(0, 1);
    } else if (e.key === 'End') {
      e.preventDefault();
      e.stopPropagation();
      activeIndex = firstEnabledFrom(options.length - 1, -1);
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      e.stopPropagation();
      commit(activeIndex);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      closeList();
    } else if (e.key === 'Tab') {
      // Commit the active option, then let focus move on naturally.
      commit(activeIndex);
    } else if (isTypeaheadKey(e)) {
      e.preventDefault();
      e.stopPropagation();
      typeahead(e.key);
    }
  }

  // Click-outside (mousedown, not click, so re-clicking the trigger doesn't
  // close-then-reopen) collapses without committing.
  $effect(() => {
    if (!open) return;
    function onMouseDown(e: MouseEvent) {
      const target = e.target as Node;
      if (trigger?.contains(target) || listEl?.contains(target)) return;
      open = false;
      activeIndex = -1;
    }
    document.addEventListener('mousedown', onMouseDown);
    return () => document.removeEventListener('mousedown', onMouseDown);
  });

  // A fixed popover does not follow the trigger on layout shift — close on
  // ancestor scroll / resize. The scroll listener is capture-phase (3rd arg
  // true) so it catches non-bubbling scroll from ancestor containers (sidebar,
  // modal body). Capture also delivers scroll from DESCENDANTS, so we must
  // ignore scrolls originating inside the popover's own list — otherwise
  // wheeling a long list, or the arrow-key scrollIntoView below, would fire
  // this and dismiss the dropdown mid-interaction.
  $effect(() => {
    if (!open) return;
    const collapse = () => {
      open = false;
      activeIndex = -1;
    };
    const onScroll = (e: Event) => {
      if (listEl?.contains(e.target as Node)) return;
      collapse();
    };
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', collapse);
    return () => {
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', collapse);
    };
  });

  // Keep the active option in view while arrowing. scrollIntoView is a no-op
  // in jsdom; guard so unit tests don't choke.
  $effect(() => {
    if (!open || activeIndex < 0 || !listEl) return;
    const el = listEl.children[activeIndex] as HTMLElement | undefined;
    el?.scrollIntoView?.({ block: 'nearest' });
  });

  // Clear any pending typeahead debounce when the component is destroyed.
  $effect(() => () => clearTimeout(typeTimer));
</script>

<button
  bind:this={trigger}
  type="button"
  {id}
  {disabled}
  data-testid={dataTestid}
  role="combobox"
  aria-haspopup="listbox"
  aria-expanded={open}
  aria-controls={listboxId}
  aria-activedescendant={activeDescendant}
  aria-label={ariaLabel}
  class="inline-flex items-center justify-between gap-2 h-8 px-3 text-sm border border-border-emphasis rounded bg-surface text-primary disabled:opacity-50 disabled:cursor-not-allowed {className}"
  onclick={() => (open ? closeList() : openList())}
  onkeydown={onKeyDown}
>
  <span class="truncate" class:text-placeholder={isPlaceholder}>{displayLabel}</span>
  <span aria-hidden="true" class="text-muted">▾</span>
</button>

{#if open}
  <ul
    bind:this={listEl}
    id={listboxId}
    role="listbox"
    tabindex="-1"
    class="fixed z-50 max-h-60 overflow-y-auto bg-surface border border-border-subtle rounded shadow-md py-1 text-sm"
    style={popoverStyle}
  >
    <!-- Keyed by `opt.value`: option values must be unique within a Select
         (a native <select> tolerates duplicate values; this control does not). -->
    {#each options as opt, i (opt.value)}
      <li
        id={optionId(i)}
        role="option"
        aria-selected={opt.value === value}
        aria-disabled={opt.disabled || undefined}
        class="px-3 py-1.5 text-primary"
        class:cursor-pointer={!opt.disabled}
        class:cursor-not-allowed={opt.disabled}
        class:opacity-50={opt.disabled}
        class:bg-accent-soft={i === activeIndex}
        class:font-medium={opt.value === value}
        onmousedown={(e) => {
          e.preventDefault();
          if (!opt.disabled) commit(i);
        }}
        onmouseenter={() => {
          if (!opt.disabled) activeIndex = i;
        }}
      >
        {opt.label}
      </li>
    {/each}
  </ul>
{/if}

<script module lang="ts">
  import type { IconName } from '$lib/ui/icons';
  export type SelectOption = {
    value: string | number;
    label: string;
    disabled?: boolean;
    icon?: IconName;
  };
</script>

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
  // aria-activedescendant (never moves DOM focus into the list). With a
  // `filter`, the focus is in the filter field while the list is open, and the
  // field carries aria-activedescendant instead (McVersionCombobox's model).

  import { Icon } from '$lib/ui/icons';
  import type { Snippet } from 'svelte';
  import { isTopmost, useLayer } from '$lib/ui/layer-stack.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { computePopoverPlacement } from './select-placement';
  import { attachPopoverDismiss } from './popover-dismiss';

  type Primitive = string | number;
  type Option = SelectOption;

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
    describedby,
    optionLeading,
    valueLeading,
    optionTrailing,
    onDeleteOption,
    onOptionContextMenu,
    filter,
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
    /** Id(s) of the helper text under the control — forwarded to the trigger's aria-describedby. */
    describedby?: string;
    // Optional leading content per option row / for the selected value.
    // Falls back to the existing `icon` rendering when not provided.
    optionLeading?: Snippet<[Option]>;
    valueLeading?: Snippet<[Option]>;
    // Optional trailing content per open-list option row (e.g. a per-row action
    // button). Right-aligned. Each <li> carries a `group` class and an
    // `is-active` class (when it is the hovered / keyboard-active row), so the
    // snippet can reveal itself with those exact Tailwind variants:
    //   class="opacity-0 group-hover:opacity-100 group-[.is-active]:opacity-100"
    // (the literal strings must appear in the consumer's file for Tailwind's JIT
    // scanner to emit them). A trailing interactive control must stop its own
    // mousedown (stopPropagation + preventDefault) so it does not also commit
    // the row; activate it on click.
    optionTrailing?: Snippet<[Option]>;
    // Keyboard parity for a trailing per-row action: when set, pressing Delete
    // while the list is open invokes it for the active option (and closes the
    // list). Generic enough for any "deletable options" list.
    onDeleteOption?: (option: Option) => void;
    // Optional right-click hook per open-list option row (e.g. the sidebar's
    // per-instance actions menu). Receives the option and the pointer
    // position; the callback owns rendering its own menu. Not offered for
    // disabled rows; without it right-click keeps its native behavior.
    onOptionContextMenu?: (
      option: Option,
      pos: { x: number; y: number },
      how: { byKeyboard: boolean },
    ) => void;
    // Opt-in filter field above the open list's rows, for a list long enough to search (the
    // sidebar instance picker above 8 instances). While the list is open the focus is in the
    // field — McVersionCombobox's model: typing narrows the rows to those whose `text` (default:
    // the label) contains the query, ignoring case; the arrows move the active row, Enter and
    // Tab take it. A letter on the closed trigger opens the list with that letter in the field
    // instead of committing a first-letter match. Every open starts with an empty field.
    filter?: {
      placeholder: string;
      ariaLabel: string;
      /** The line shown when no row matches, given the trimmed query. */
      noMatches: (query: string) => string;
      text?: (option: Option) => string;
    };
  } = $props();

  const MAX_POPOVER_HEIGHT = 240; // preferred cap; clamped down to viewport room
  const GAP = 4;
  const MARGIN = 8;

  let open = $state(false);
  let activeIndex = $state(-1);
  let trigger: HTMLButtonElement | undefined = $state();
  let listEl: HTMLUListElement | undefined = $state();
  // The fixed popover: the filter field (when there is one) above the list.
  let popoverEl: HTMLDivElement | undefined = $state();
  let query = $state('');
  let popoverTop = $state(0);
  let popoverLeft = $state(0);
  let popoverWidth = $state(0);
  let popoverMaxWidth = $state(0);
  let popoverMaxHeight = $state(MAX_POPOVER_HEIGHT);
  let flipUp = $state(false);

  // Unique ids so multiple Selects can coexist (filter rows stack several).
  const uid = crypto.randomUUID();
  const listboxId = `select-list-${uid}`;
  const optionId = (i: number) => `select-opt-${uid}-${i}`;

  const selectedIndex = $derived(options.findIndex((o) => o.value === value));
  const selectedLabel = $derived(selectedIndex >= 0 ? options[selectedIndex].label : null);
  const selectedOption = $derived(selectedIndex >= 0 ? options[selectedIndex] : undefined);
  const displayLabel = $derived(selectedLabel ?? placeholder ?? '');
  // Grey the trigger when nothing meaningful is chosen: no matching option,
  // or an explicit empty value (the "Any…" / "Choose…" rows filters keep).
  const isPlaceholder = $derived(selectedLabel === null || value === '' || value === null);
  const activeDescendant = $derived(open && activeIndex >= 0 ? optionId(activeIndex) : undefined);
  const needle = $derived(filter ? query.trim().toLowerCase() : '');
  // Indices into `options` of the rows shown: every one, unless a filter query narrows them.
  const visible = $derived(
    needle === ''
      ? options.map((_, i) => i)
      : options.flatMap((o, i) =>
          (filter?.text?.(o) ?? o.label).toLowerCase().includes(needle) ? [i] : [],
        ),
  );
  const popoverStyle = $derived(
    (flipUp ? `bottom: ${window.innerHeight - popoverTop}px;` : `top: ${popoverTop}px;`) +
      ` left: ${popoverLeft}px; min-width: ${popoverWidth}px; max-width: ${popoverMaxWidth}px;` +
      ` max-height: ${popoverMaxHeight}px;`,
  );

  // `listWidth`: the open list's own width, once it has been laid out (see the effect below).
  function positionPopover(listWidth?: number) {
    if (!trigger) return;
    const r = trigger.getBoundingClientRect();
    // Clamp the list to whatever vertical room the viewport actually has. In
    // compact mode the OS window shrinks to the sidebar's content height, so a
    // position:fixed popover lives in a short viewport — without clamping its
    // height (and scrolling internally) it spills past the window edge and gets
    // clipped. See computePopoverPlacement for the maths.
    const p = computePopoverPlacement(
      { top: r.top, bottom: r.bottom, left: r.left, width: r.width },
      { width: window.innerWidth, height: window.innerHeight },
      { gap: GAP, margin: MARGIN, maxHeight: MAX_POPOVER_HEIGHT, popoverWidth: listWidth },
    );
    flipUp = p.flipUp;
    popoverTop = p.top;
    popoverLeft = p.left;
    popoverWidth = p.width;
    popoverMaxWidth = p.maxWidth;
    popoverMaxHeight = p.maxHeight;
  }

  // Walk the shown rows from position `pos` in direction `dir` to the first non-disabled one;
  // its index into `options`, or -1.
  function enabledFrom(pos: number, dir: 1 | -1): number {
    for (let p = pos; p >= 0 && p < visible.length; p += dir) {
      if (!options[visible[p]].disabled) return visible[p];
    }
    return -1;
  }

  /** The active row's position among the shown rows (-1 when none is active). */
  const activePos = () => visible.indexOf(activeIndex);

  // `initialQuery`: the letter typed on the closed trigger of a filtering list.
  function openList(initialQuery = '') {
    if (disabled) return;
    query = initialQuery;
    editMenuAbove = false;
    positionPopover();
    open = true;
    activeIndex =
      !initialQuery && selectedIndex >= 0 && !options[selectedIndex].disabled
        ? selectedIndex
        : enabledFrom(0, 1);
  }

  function closeList() {
    open = false;
    activeIndex = -1;
    trigger?.focus();
  }

  // Each edit of the filter makes the first shown row active, so Enter takes the top match.
  function setQuery(q: string) {
    query = q;
    activeIndex = enabledFrom(0, 1);
  }

  // The filter field takes the focus as it appears; closing gives it back to the trigger.
  function focusOnMount(node: HTMLInputElement) {
    node.focus();
  }

  // The open list is a layer in the app's layer stack: a contextual tour running
  // underneath steps aside while it is open, and an Escape that reaches the
  // router (focus elsewhere) closes the list, not the dialog around it.
  const layerId = useLayer('popover', () => open, closeList);

  // A right-click in the filter field opens the app's edit menu (+layout) ABOVE the list. A press
  // in that menu is not a press outside the list: closing on it would unmount the field before
  // Cut/Copy/Paste lands. Only that menu: the sidebar's row menu, also above the list, is meant to
  // close it, as it always has.
  let editMenuAbove = false;

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
        if (filter) openList(e.key);
        else typeahead(e.key);
      }
      return;
    }
    onOpenListKey(e, false);
  }

  // Keys while the list is open, on the trigger or (`inField`) in the filter field. Keys
  // consumed here are stopped from propagating, so the layer router (Escape-to-close of an
  // enclosing modal, etc.) doesn't also fire — pressing Escape should dismiss only the
  // dropdown, matching native <select>. Tab is the exception: it commits but must keep
  // bubbling/default so focus moves on to the next control. In the field, the keys that edit
  // text — letters, Space, Home/End, Delete — keep their default.
  function onOpenListKey(e: KeyboardEvent, inField: boolean) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      e.stopPropagation();
      const next = enabledFrom(activePos() + 1, 1);
      if (next >= 0) activeIndex = next;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      e.stopPropagation();
      const prev = enabledFrom(activePos() - 1, -1);
      if (prev >= 0) activeIndex = prev;
    } else if (!inField && e.key === 'Home') {
      e.preventDefault();
      e.stopPropagation();
      activeIndex = enabledFrom(0, 1);
    } else if (!inField && e.key === 'End') {
      e.preventDefault();
      e.stopPropagation();
      activeIndex = enabledFrom(visible.length - 1, -1);
    } else if (e.key === 'Enter' || (!inField && e.key === ' ')) {
      e.preventDefault();
      e.stopPropagation();
      commit(activeIndex);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      closeList();
    } else if (!inField && e.key === 'Delete' && onDeleteOption && activeIndex >= 0) {
      e.preventDefault();
      e.stopPropagation();
      const opt = options[activeIndex];
      closeList();
      onDeleteOption(opt);
    } else if (
      onOptionContextMenu &&
      activeIndex >= 0 &&
      (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10'))
    ) {
      // The keyboard twin of a right-click on the active row. The list closes
      // first (which refocuses the trigger), so the menu that opens can return
      // focus somewhere real; the key's default would open the WebView menu.
      e.preventDefault();
      e.stopPropagation();
      const opt = options[activeIndex];
      const r = document.getElementById(optionId(activeIndex))?.getBoundingClientRect();
      closeList();
      onOptionContextMenu(
        opt,
        { x: r ? r.left + 8 : 8, y: r ? r.bottom : 8 },
        { byKeyboard: true },
      );
    } else if (e.key === 'Tab') {
      // Commit the active option, then let focus move on naturally — from the trigger, which
      // commit (or, with no row to take, closeList) has just focused.
      if (activeIndex >= 0) commit(activeIndex);
      else closeList();
    } else if (!inField && isTypeaheadKey(e)) {
      e.preventDefault();
      e.stopPropagation();
      if (filter) setQuery(query + e.key);
      else typeahead(e.key);
    }
  }

  // Click-outside (mousedown, not click, so re-clicking the trigger doesn't
  // close-then-reopen) collapses without committing.
  $effect(() => {
    if (!open) return;
    function onMouseDown(e: MouseEvent) {
      const target = e.target as Node;
      if (trigger?.contains(target) || popoverEl?.contains(target)) return;
      if (editMenuAbove && !isTopmost(layerId)) return;
      editMenuAbove = false;
      open = false;
      activeIndex = -1;
    }
    document.addEventListener('mousedown', onMouseDown);
    return () => document.removeEventListener('mousedown', onMouseDown);
  });

  // The list is as wide as its longest option, which the trigger does not know: placed by the
  // trigger's width, the Installed sort list ran 56 px past an 820 px window (plan §5e). Once the
  // list has been laid out it is placed again by its own width — in the same flush, so before the
  // frame is painted, as Menu does.
  $effect(() => {
    if (open && popoverEl) positionPopover(popoverEl.offsetWidth);
  });

  // A fixed popover does not follow the trigger on layout shift — close on
  // ancestor scroll / resize (shared helper; ignoreScrollWithin keeps wheeling
  // the list or the arrow-key scrollIntoView below from dismissing it).
  $effect(() => {
    if (!open) return;
    return attachPopoverDismiss({
      onDismiss: () => {
        open = false;
        activeIndex = -1;
      },
      // The popover, not just the list: a query longer than the field scrolls the field.
      ignoreScrollWithin: () => popoverEl,
    });
  });

  // Keep the active option in view while arrowing. scrollIntoView is a no-op
  // in jsdom; guard so unit tests don't choke.
  $effect(() => {
    if (!open || activeIndex < 0 || !listEl) return;
    const el = document.getElementById(optionId(activeIndex));
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
  aria-activedescendant={filter ? undefined : activeDescendant}
  aria-label={ariaLabel}
  aria-describedby={describedby}
  class="inline-flex items-center justify-between gap-2 h-8 px-3 text-sm border border-border-emphasis rounded bg-surface text-primary disabled:opacity-50 disabled:cursor-not-allowed disabled:bg-subtle disabled:text-muted {className}"
  onclick={() => (open ? closeList() : openList())}
  onkeydown={onKeyDown}
>
  <span class="inline-flex items-center gap-2 min-w-0">
    {#if valueLeading && selectedOption}
      {@render valueLeading(selectedOption)}
    {:else if selectedOption?.icon}
      <Icon name={selectedOption.icon} class="flex-shrink-0" />
    {/if}
    <span
      class="truncate"
      class:text-placeholder={isPlaceholder}
      use:tooltip={{ text: displayLabel, whenOverflowing: true }}>{displayLabel}</span
    >
  </span>
  <Icon name="chevronDown" class="text-muted flex-shrink-0" />
</button>

{#if open}
  <!-- A press on the popover outside the field and the rows (its padding, the no-match line)
       must not take the focus out of the field. -->
  <div
    bind:this={popoverEl}
    role="presentation"
    class="fixed z-[var(--z-popover)] flex flex-col bg-surface border border-border-subtle rounded shadow-md text-sm"
    style={popoverStyle}
    onmousedown={(e) => {
      if (filter && !(e.target instanceof HTMLInputElement)) e.preventDefault();
    }}
  >
    {#if filter}
      <div class="flex-shrink-0 border-b border-border-subtle p-1.5">
        <input
          use:focusOnMount
          type="search"
          role="combobox"
          aria-expanded={visible.length > 0}
          aria-controls={listboxId}
          aria-autocomplete="list"
          aria-activedescendant={activeDescendant}
          aria-label={filter.ariaLabel}
          placeholder={filter.placeholder}
          autocomplete="off"
          spellcheck="false"
          class="w-full rounded border border-border-emphasis bg-surface px-2 py-1 text-sm text-primary"
          value={query}
          oninput={(e) => setQuery(e.currentTarget.value)}
          oncontextmenu={() => (editMenuAbove = true)}
          onkeydown={(e) => onOpenListKey(e, true)}
        />
      </div>
    {/if}
    <ul
      bind:this={listEl}
      id={listboxId}
      role="listbox"
      tabindex="-1"
      class="min-h-0 overflow-y-auto py-1"
      class:hidden={visible.length === 0}
    >
      <!-- Keyed by `opt.value`: option values must be unique within a Select
           (a native <select> tolerates duplicate values; this control does not). -->
      {#each visible as i (options[i].value)}
        {@const opt = options[i]}
        <li
          id={optionId(i)}
          role="option"
          aria-selected={opt.value === value}
          aria-disabled={opt.disabled || undefined}
          class="group flex items-center gap-2 px-3 py-1.5 text-primary"
          class:is-active={i === activeIndex}
          class:cursor-pointer={!opt.disabled}
          class:cursor-not-allowed={opt.disabled}
          class:opacity-50={opt.disabled}
          class:bg-accent-soft={i === activeIndex}
          class:font-medium={opt.value === value}
          onmousedown={(e) => {
            // Right-button presses belong to the context-menu hook (when set):
            // they must neither commit nor close the list.
            if (onOptionContextMenu && e.button === 2) return;
            e.preventDefault();
            if (!opt.disabled) commit(i);
          }}
          oncontextmenu={(e) => {
            if (!onOptionContextMenu || opt.disabled) return;
            e.preventDefault();
            e.stopPropagation();
            onOptionContextMenu(opt, { x: e.clientX, y: e.clientY }, { byKeyboard: false });
          }}
          onmouseenter={() => {
            if (!opt.disabled) activeIndex = i;
          }}
        >
          <span class="inline-flex min-w-0 flex-1 items-center gap-2">
            {#if optionLeading}
              {@render optionLeading(opt)}
            {:else if opt.icon}
              <Icon name={opt.icon} class="flex-shrink-0" />
            {/if}
            <span class="truncate">{opt.label}</span>
          </span>
          {#if optionTrailing}
            {@render optionTrailing(opt)}
          {/if}
        </li>
      {/each}
    </ul>
    {#if filter}
      <!-- Mounted with the field, so the no-match line is announced when it appears; out of flow
           while idle (DESIGN §10, tests/no-idle-live-region-box.test.ts). `w-0
           min-w-full`: it wraps inside the popover instead of widening it as the query grows. -->
      <p
        role="status"
        class="w-0 min-w-full text-xs text-muted"
        class:absolute={visible.length > 0}
        class:px-3={visible.length === 0}
        class:py-2={visible.length === 0}
      >
        {#if visible.length === 0}{filter.noMatches(query.trim())}{/if}
      </p>
    {/if}
  </div>
{/if}

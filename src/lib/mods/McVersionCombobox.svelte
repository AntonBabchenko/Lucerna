<script lang="ts">
  import { tick } from 'svelte';
  import { mcVersions } from '$lib/settings/state.svelte';
  import { t } from '$lib/i18n';
  import { useLayer } from '$lib/ui/layer-stack.svelte';
  import { attachPopoverDismiss } from '$lib/ui/popover-dismiss';
  import { computePopoverPlacement } from '$lib/ui/select-placement';

  // A combobox over the Minecraft version list: text input that opens a
  // filtered dropdown below it. Lives in $lib/mods because the only two
  // call sites (ModBrowseView + ModpackBrowseView) sit nearby; rehome
  // if a third unrelated caller appears.
  //
  // Empty `value` means "no version filter" — the caller sends null to
  // the search backend. A "Any version" row at the top of the dropdown
  // is the explicit way to reset.
  //
  // A11y (DESIGN.md "Known gaps"): `aria-selected` marks the COMMITTED option
  // (the row whose id === value, or the "Any version" row when value is empty),
  // NOT the keyboard-active row — the active descendant is already announced via
  // `aria-activedescendant`. Mapping aria-selected to activeIndex made a screen
  // reader read whatever row the cursor sat on as "selected", so the user could
  // never hear which version was actually chosen. Home/End jump to first/last
  // match, matching Select.

  let {
    value = $bindable(''),
    placeholder = 'MC version',
    id,
    dataTestid,
    disabled = false,
    describedby = undefined,
    invalid = false,
  }: {
    value?: string;
    placeholder?: string;
    id?: string;
    dataTestid?: string;
    /** Ids of what describes the field (a refusal under it), like Select's `describedby`. */
    describedby?: string;
    /** The value was refused: `aria-invalid`. */
    invalid?: boolean;
    // When disabled (e.g. an FTB source that can't server-filter by MC), the
    // input rejects input/focus so the dropdown never opens and `value` can't be
    // mutated into a filter the backend would ignore.
    disabled?: boolean;
  } = $props();

  let open = $state(false);
  let activeIndex = $state(-1);
  let inputEl: HTMLInputElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();
  // aria-controls needs a stable id on the listbox. Unique per
  // instance — important if multiple comboboxes coexist (ModBrowseView
  // + ModpackBrowseView's filters share the same view tree in some
  // layouts).
  const listboxId = `mc-combobox-list-${crypto.randomUUID()}`;
  // Stable per-option ids so `aria-activedescendant` on the input can point at
  // the arrow-key-highlighted option — without it, keyboard navigation is
  // invisible to a screen reader (the input's value never changes on ArrowUp/Down).
  const optionId = (i: number) => `${listboxId}-opt-${i}`;
  // Stable id for the "Any version" reset row so aria-selected can mark it as
  // the committed option when no filter is set (value === '').
  const anyOptionId = `${listboxId}-any`;

  // Snapshots / pre-releases would bury the stable list. The Manage
  // modal already excludes them from the version picker — same call
  // here.
  const releases = $derived(
    mcVersions.value.filter((v) => v.version_type === 'release').map((v) => v.id),
  );
  // Treat the current input as a live filter against the release list.
  // If the input is empty, show everything.
  const filtered = $derived(
    value ? releases.filter((id) => id.toLowerCase().includes(value.toLowerCase())) : releases,
  );

  function pick(v: string) {
    value = v;
    open = false;
    activeIndex = -1;
    inputEl?.blur();
  }

  function clear() {
    value = '';
    open = false;
    activeIndex = -1;
    inputEl?.blur();
  }

  function onKeyDown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      open = true;
      activeIndex = Math.min(activeIndex + 1, filtered.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      activeIndex = Math.max(activeIndex - 1, -1);
    } else if (e.key === 'Home') {
      // Parity with Select: jump to the first match.
      e.preventDefault();
      open = true;
      activeIndex = filtered.length > 0 ? 0 : -1;
    } else if (e.key === 'End') {
      // Parity with Select: jump to the last match.
      e.preventDefault();
      open = true;
      activeIndex = filtered.length - 1;
    } else if (e.key === 'Enter') {
      if (activeIndex >= 0 && activeIndex < filtered.length) {
        e.preventDefault();
        pick(filtered[activeIndex]);
      } else {
        open = false;
      }
    } else if (e.key === 'Escape' && open) {
      // Closing the list is this press's whole job: consume it, so the layer
      // router leaves the dialog or tour underneath alone. "The list is still
      // the top layer when the router runs" cannot be relied on: an effect
      // releases its layer, and for a real key press the browser runs pending
      // microtasks — Svelte's flush among them — between this listener and
      // the router's. With the list closed, the key goes on as usual.
      e.preventDefault();
      open = false;
      activeIndex = -1;
    }
  }

  // The open dropdown is a layer in the app's layer stack: a contextual tour
  // underneath steps aside while it is open, and an Escape that reaches the
  // router while the list is on top closes the list.
  useLayer(
    'popover',
    () => open,
    () => {
      open = false;
      activeIndex = -1;
    },
  );

  // Click-outside collapses the dropdown without committing the
  // highlighted row. mousedown (not click) so re-clicking the input
  // doesn't immediately close-then-reopen.
  $effect(() => {
    if (!open) return;
    function onMouseDown(e: MouseEvent) {
      const t = e.target as Node;
      if (inputEl?.contains(t) || listEl?.contains(t)) return;
      open = false;
      activeIndex = -1;
    }
    document.addEventListener('mousedown', onMouseDown);
    return () => document.removeEventListener('mousedown', onMouseDown);
  });

  // The list is placed as Select's is (select-placement.ts): position:fixed, under the input — or
  // over it when the room below is short — as tall as the room allows, and kept inside the window
  // by its own width. As `absolute` it hung 16 px past the input's right edge, 4 px past the
  // launcher's 820 px window, and the scroll container cut its border off (plan §5e).
  const LIST_MAX_HEIGHT = 240; // the old max-h-60
  const GAP = 4; // the old mt-1
  const MARGIN = 8;
  let listTop = $state(0);
  let listLeft = $state(0);
  let listMaxHeight = $state(LIST_MAX_HEIGHT);
  let flipUp = $state(false);
  const listStyle = $derived(
    (flipUp ? `bottom: ${window.innerHeight - listTop}px;` : `top: ${listTop}px;`) +
      ` left: ${listLeft}px; max-height: ${listMaxHeight}px;`,
  );

  function placeList(listWidth: number) {
    if (!inputEl) return;
    const r = inputEl.getBoundingClientRect();
    const p = computePopoverPlacement(
      { top: r.top, bottom: r.bottom, left: r.left, width: r.width },
      { width: window.innerWidth, height: window.innerHeight },
      { gap: GAP, margin: MARGIN, maxHeight: LIST_MAX_HEIGHT, popoverWidth: listWidth },
    );
    flipUp = p.flipUp;
    listTop = p.top;
    listLeft = p.left;
    listMaxHeight = p.maxHeight;
  }

  // Placed once it has been laid out, its width known — in the same flush, before the frame is
  // painted (Select and Menu do the same) — and placed again after every keystroke while open. A
  // new value is a new filter, and the row around the field may answer it by moving the field: in
  // the Browse filter bar "Match this instance" or "Clear all" comes in, the search box gives up
  // the room, and the field moves left (110 px for "Match this instance" in a 1600 px window). The
  // `absolute` list moved with it; a fixed one stays where it was put. The second measurement
  // waits for `tick`, so a row that answers in an update of its own (an `$effect`) has answered
  // too — still before the frame is painted, as `tick` is a microtask while Svelte's experimental
  // async mode is off. `live` drops it once the list closes or the next keystroke comes.
  $effect(() => {
    void value;
    if (!open || !listEl) return;
    placeList(listEl.offsetWidth);
    let live = true;
    void tick().then(() => {
      if (live && open && listEl) placeList(listEl.offsetWidth);
    });
    return () => {
      live = false;
    };
  });

  // And placed again whenever the box the field is laid out in changes size without a keystroke:
  // results reloading under the Browse filter bar shortened the page, its scroll container dropped
  // the scrollbar and the bar widened by 15 px, then narrowed as the results came back ~200 ms
  // later — moving the field and leaving the list behind. The scroll containers reserve the
  // scrollbar's gutter now (DESIGN.md §14); this covers a platform without `scrollbar-gutter`, and
  // any other change to that box's width. A ResizeObserver answers after layout, before the paint.
  $effect(() => {
    if (!open || !inputEl || !listEl || typeof ResizeObserver === 'undefined') return;
    const box = inputEl.offsetParent;
    if (!(box instanceof HTMLElement)) return;
    const list = listEl;
    const observer = new ResizeObserver(() => placeList(list.offsetWidth));
    observer.observe(box);
    return () => observer.disconnect();
  });

  // Fixed, it does not follow the input when the page scrolls or the window resizes: it closes.
  // Scrolling the list itself does not.
  $effect(() => {
    if (!open) return;
    return attachPopoverDismiss({
      onDismiss: () => {
        open = false;
        activeIndex = -1;
      },
      ignoreScrollWithin: () => listEl,
    });
  });
</script>

<div>
  <input
    bind:this={inputEl}
    {id}
    data-testid={dataTestid}
    type="text"
    {placeholder}
    {disabled}
    bind:value
    oninput={() => {
      if (disabled) return;
      open = true;
      activeIndex = -1;
    }}
    onfocus={() => {
      if (disabled) return;
      open = true;
    }}
    onkeydown={onKeyDown}
    class="filter-control filter-control-narrow disabled:opacity-50 disabled:cursor-not-allowed"
    autocomplete="off"
    role="combobox"
    aria-autocomplete="list"
    aria-controls={listboxId}
    aria-expanded={open}
    aria-describedby={describedby}
    aria-invalid={invalid ? 'true' : undefined}
    aria-activedescendant={open && activeIndex >= 0 && activeIndex < filtered.length
      ? optionId(activeIndex)
      : undefined}
  />
  {#if open}
    <div
      id={listboxId}
      bind:this={listEl}
      role="listbox"
      class="fixed z-[var(--z-popover)] w-32 overflow-y-auto bg-surface border border-border-subtle rounded shadow"
      style={listStyle}
    >
      <button
        type="button"
        id={anyOptionId}
        role="option"
        aria-selected={value === ''}
        class="block w-full text-left px-3 py-1 btn-tertiary text-sm italic"
        onclick={clear}
      >
        {$t('mods.mcVersion.anyVersion')}
      </button>
      {#each filtered as id, i (id)}
        <button
          type="button"
          id={optionId(i)}
          role="option"
          aria-selected={id === value}
          class="block w-full text-left px-3 py-1 text-sm hover:bg-subtle"
          class:bg-accent-soft={activeIndex === i}
          onclick={() => pick(id)}
        >
          {id}
        </button>
      {/each}
      {#if filtered.length === 0}
        <div class="px-3 py-2 text-xs text-muted">{$t('mods.mcVersion.noMatch')}</div>
      {/if}
    </div>
  {/if}
</div>

<script lang="ts">
  import { mcVersions } from '$lib/settings/state.svelte';
  import { t } from '$lib/i18n';

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
  }: {
    value?: string;
    placeholder?: string;
    id?: string;
    dataTestid?: string;
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
    } else if (e.key === 'Escape') {
      open = false;
      activeIndex = -1;
    }
  }

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
</script>

<div class="relative">
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
    aria-activedescendant={open && activeIndex >= 0 && activeIndex < filtered.length
      ? optionId(activeIndex)
      : undefined}
  />
  {#if open}
    <div
      id={listboxId}
      bind:this={listEl}
      role="listbox"
      class="absolute z-30 mt-1 w-32 max-h-60 overflow-y-auto bg-surface border border-border-subtle rounded shadow"
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

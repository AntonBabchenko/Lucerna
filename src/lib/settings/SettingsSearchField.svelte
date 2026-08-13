<script lang="ts">
  // Settings search: the input plus a section-grouped results listbox. Owns the
  // query, the derived matches, and roving keyboard selection. When the query is
  // non-empty it sets `searching` (bindable) so the modal hides the 7-section
  // tablist and shows these results instead. Emits onselect(entry); the modal
  // switches section and flashes the control.
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { Icon } from '$lib/ui/icons';
  import { searchSettings } from './search';
  import { SETTINGS_ENTRIES, type SettingsSearchEntry } from './search-index';

  let {
    searching = $bindable(false),
    onselect,
  }: { searching?: boolean; onselect: (entry: SettingsSearchEntry) => void } = $props();

  let query = $state('');
  let activeIndex = $state(0);

  const results = $derived(searchSettings(query, SETTINGS_ENTRIES, $t));

  $effect(() => {
    searching = query.trim().length > 0;
  });

  // New query ⇒ active row back to the top (so Enter hits the first match).
  $effect(() => {
    void query;
    activeIndex = 0;
  });

  $effect(() => {
    if (activeIndex > results.length - 1) activeIndex = 0;
  });

  const optionId = (i: number): string => `settings-search-opt-${i}`;

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown' && results.length > 0) {
      activeIndex = Math.min(results.length - 1, activeIndex + 1);
      e.preventDefault();
    } else if (e.key === 'ArrowUp' && results.length > 0) {
      activeIndex = Math.max(0, activeIndex - 1);
      e.preventDefault();
    } else if (e.key === 'Enter') {
      const hit = results[activeIndex];
      if (hit) {
        onselect(hit);
        e.preventDefault();
      }
    } else if (e.key === 'Escape' && query.length > 0) {
      // Clear the query first; only if already empty let it bubble to close the modal.
      query = '';
      e.preventDefault();
      e.stopPropagation();
    }
  }
</script>

<div class="p-2 border-b shrink-0">
  <div class="relative">
    <span class="absolute left-2 top-1/2 -translate-y-1/2 text-muted pointer-events-none">
      <Icon name="search" size={14} />
    </span>
    <!-- Combobox pattern: a text input that owns keyboard control of the results
         listbox via aria-activedescendant (the option rows are never focused
         themselves), so role="combobox" — not the input's implicit searchbox. -->
    <input
      type="text"
      role="combobox"
      class="w-full text-sm border rounded pl-7 pr-2 py-1.5"
      aria-label={$t('settings.search.label')}
      aria-expanded={searching}
      aria-controls={searching ? 'settings-search-results' : undefined}
      aria-autocomplete="list"
      aria-activedescendant={searching && results.length > 0 ? optionId(activeIndex) : undefined}
      placeholder={$t('settings.search.placeholder')}
      bind:value={query}
      onkeydown={onKeydown}
      data-testid="settings-search-input"
    />
  </div>
</div>

{#if searching}
  <ul
    id="settings-search-results"
    role="listbox"
    aria-label={$t('settings.search.label')}
    class="p-2 flex flex-col gap-0.5 overflow-y-auto"
    data-testid="settings-search-results"
  >
    {#if results.length === 0}
      <li class="px-3 py-2 text-sm text-muted" data-testid="settings-search-empty">
        {$t('settings.search.noResults')}
      </li>
    {:else}
      {#each results as entry, i (entry.anchor)}
        {#if i === 0 || results[i - 1].tab !== entry.tab}
          <li role="presentation" class="px-3 pt-2 pb-1 text-xs uppercase tracking-wide text-muted">
            {$t(`settings.sections.${entry.tab}` as TranslationKey)}
          </li>
        {/if}
        <!-- Keyboard is delegated to the combobox input (aria-activedescendant),
             so these option rows are pointer-only by design. -->
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
        <li
          id={optionId(i)}
          role="option"
          aria-selected={i === activeIndex}
          class="text-left px-3 py-1.5 text-sm rounded border-l-2 cursor-pointer"
          class:border-accent={i === activeIndex}
          class:text-primary={i === activeIndex}
          class:font-medium={i === activeIndex}
          class:border-transparent={i !== activeIndex}
          class:text-muted={i !== activeIndex}
          onclick={() => onselect(entry)}
          onmousemove={() => (activeIndex = i)}
          data-search-result={entry.anchor}
        >
          {$t(entry.labelKey)}
        </li>
      {/each}
    {/if}
  </ul>
{/if}

<script lang="ts">
  import type { LoaderKind, ModSource } from '$lib/ipc/bindings';
  import McVersionCombobox from '$lib/mods/McVersionCombobox.svelte';
  import SegmentedControl from './SegmentedControl.svelte';

  // Right-side overlay sheet holding the facet fields. Facet state is
  // owned by the parent and bound here. `source` and `showInstalled` are
  // optional fields: a field renders only when its prop is supplied
  // (source = modpack browser; showInstalled = mod browser). Loader and
  // MC are always present. Apply is live — binding a value mutates the
  // parent rune, whose existing search effect re-runs.
  let {
    open = $bindable(false),
    loader = $bindable<LoaderKind | ''>(''),
    mc = $bindable(''),
    mcTestid,
    source = $bindable<ModSource | undefined>(undefined),
    showInstalled = undefined,
    onShowInstalledChange,
  }: {
    open?: boolean;
    loader?: LoaderKind | '';
    mc?: string;
    mcTestid?: string;
    source?: ModSource | undefined;
    // showInstalled is a controlled value + callback (not bound) because
    // the mod browser needs bespoke re-paging when it flips.
    showInstalled?: boolean | undefined;
    onShowInstalledChange?: (value: boolean) => void;
  } = $props();

  const LOADER_OPTIONS = [
    { value: '', label: 'Any' },
    { value: 'fabric', label: 'Fabric' },
    { value: 'quilt', label: 'Quilt' },
    { value: 'forge', label: 'Forge' },
    { value: 'neoforge', label: 'NeoForge' },
  ];
  const SOURCE_OPTIONS = [
    { value: 'modrinth', label: 'Modrinth' },
    { value: 'curseforge', label: 'CurseForge' },
  ];

  let panelEl: HTMLDivElement | undefined = $state();

  // On open: remember what had focus, move focus into the panel, and wire
  // Escape-to-close. On close: remove the listener and restore focus to
  // the element that opened the drawer (typically the Filters trigger) so
  // keyboard / screen-reader users don't lose their place.
  $effect(() => {
    if (!open) return;
    const previouslyFocused = document.activeElement as HTMLElement | null;
    panelEl?.focus();
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') open = false;
    }
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('keydown', onKey);
      previouslyFocused?.focus();
    };
  });
</script>

{#if open}
  <div class="fixed inset-0 z-40">
    <button
      type="button"
      class="absolute inset-0 bg-black/30"
      aria-label="Close filters overlay"
      onclick={() => (open = false)}
    ></button>
    <div
      bind:this={panelEl}
      tabindex="-1"
      role="dialog"
      aria-modal="true"
      aria-label="Filters"
      data-testid="browse-filter-drawer"
      class="absolute right-0 top-0 bottom-0 w-72 bg-surface border-l border-border-subtle shadow-xl p-4 overflow-y-auto flex flex-col gap-4"
    >
      <div class="flex items-center justify-between">
        <h2 class="text-sm font-semibold text-primary">Filters</h2>
        <button
          type="button"
          class="btn-icon"
          aria-label="Close filters"
          data-testid="browse-filter-drawer-close"
          onclick={() => (open = false)}>✕</button
        >
      </div>

      {#if source !== undefined}
        <div class="flex flex-col gap-1">
          <span class="text-xs uppercase tracking-wide text-placeholder">Source</span>
          <SegmentedControl
            value={source}
            options={SOURCE_OPTIONS}
            ariaLabel="Mod source"
            testid="browse-source-segment"
            onChange={(v) => (source = v as ModSource)}
          />
        </div>
      {/if}

      <div class="flex flex-col gap-1">
        <span class="text-xs uppercase tracking-wide text-placeholder">Mod loader</span>
        <SegmentedControl
          value={loader}
          options={LOADER_OPTIONS}
          ariaLabel="Loader filter"
          testid="browse-loader-segment"
          onChange={(v) => (loader = v as LoaderKind | '')}
        />
      </div>

      <div class="flex flex-col gap-1">
        <span class="text-xs uppercase tracking-wide text-placeholder">Minecraft version</span>
        <McVersionCombobox bind:value={mc} dataTestid={mcTestid} placeholder="Any" />
      </div>

      {#if showInstalled !== undefined && onShowInstalledChange}
        <label class="flex items-center gap-2 text-sm text-secondary">
          <input
            type="checkbox"
            checked={showInstalled}
            onchange={(e) => onShowInstalledChange(e.currentTarget.checked)}
          />
          Show installed
        </label>
      {/if}
    </div>
  </div>
{/if}

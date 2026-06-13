<script lang="ts">
  import type { InstalledMod, ModSummary } from '$lib/ipc/bindings';
  import type { IconName } from '$lib/ui/icons';
  import { browserPrefs } from './browser-prefs.svelte';
  import ModCard from './ModCard.svelte';

  // Renders the list of ModCards in the correct wrapper for the chosen layout.
  // Extracts the grid/list duplication that previously existed twice in
  // ModBrowseView — the only difference between the two blocks was the
  // wrapper element/classes and the `layout` prop passed to each ModCard.

  let {
    hits,
    layout,
    isMod,
    placeholderIcon,
    installedFor,
    isCardBusy,
    onInstall,
    onOpenDetail,
    onToggle,
    onUninstall,
  }: {
    hits: ModSummary[];
    layout: 'grid' | 'list';
    isMod: boolean;
    placeholderIcon: IconName;
    installedFor: (h: ModSummary) => InstalledMod | null;
    isCardBusy: (id: string) => boolean;
    onInstall: (h: ModSummary) => void;
    onOpenDetail: (h: ModSummary) => void;
    onToggle: (h: ModSummary) => void;
    onUninstall: (h: ModSummary) => void;
  } = $props();
</script>

{#if layout === 'grid'}
  <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2 items-stretch">
    {#each hits as hit (`${hit.source}:${hit.project_id}`)}
      <ModCard
        summary={hit}
        installed={installedFor(hit)}
        installing={isCardBusy(hit.project_id)}
        onInstall={() => onInstall(hit)}
        onOpenDetail={() => onOpenDetail(hit)}
        onToggle={() => onToggle(hit)}
        onUninstall={() => onUninstall(hit)}
        canToggle={isMod}
        {placeholderIcon}
        layout="grid"
        density={browserPrefs.density}
      />
    {/each}
  </div>
{:else}
  <div class="mt-1 flex flex-col border border-border-subtle rounded overflow-hidden">
    {#each hits as hit (`${hit.source}:${hit.project_id}`)}
      <ModCard
        summary={hit}
        installed={installedFor(hit)}
        installing={isCardBusy(hit.project_id)}
        onInstall={() => onInstall(hit)}
        onOpenDetail={() => onOpenDetail(hit)}
        onToggle={() => onToggle(hit)}
        onUninstall={() => onUninstall(hit)}
        canToggle={isMod}
        {placeholderIcon}
        layout="list"
        density={browserPrefs.density}
      />
    {/each}
  </div>
{/if}

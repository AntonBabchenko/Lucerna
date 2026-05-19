<script lang="ts">
  import type { ModSource } from '$lib/ipc/bindings';
  import { modBrowserNav } from '$lib/settings/state.svelte';
  import InstalledModsView from './InstalledModsView.svelte';
  import ModBrowseView from './ModBrowseView.svelte';
  import SourcePicker from './SourcePicker.svelte';

  type View = 'browse' | 'installed';

  let view = $state<View>('browse');
  let source = $state<ModSource>('modrinth');

  // Once a sub-tab is opened we keep it mounted (hidden via CSS when
  // not active) so the user's filters, search query, pagination etc.
  // survive switching back and forth. Without this each switch
  // re-mounts the view component and resets its $state.
  let browseMounted = $state(true); // Browse is the default — mount immediately.
  let installedMounted = $state(false);
  $effect(() => {
    if (view === 'browse') browseMounted = true;
    if (view === 'installed') installedMounted = true;
  });

  // Cross-component navigation from Overview: open the Installed
  // sub-view directly. Resets the rune so subsequent in-tab clicks
  // aren't hijacked.
  $effect(() => {
    if (modBrowserNav.value !== null) {
      view = modBrowserNav.value.view;
      modBrowserNav.value = null;
    }
  });

  // Props come from +page.svelte's activeInstance and are forwarded to
  // ModBrowseView (Task 14) and InstalledModsView (Task 17). When no
  // instance is selected the Browse pane still works for read-only
  // browsing — only Install needs all three, and InstalledModsView
  // renders its own "Pick an instance first" empty state.
  let {
    instanceId,
    mcVersion,
    loader,
  }: {
    instanceId: string | null;
    mcVersion: string | null;
    loader: 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;
  } = $props();
</script>

<div class="flex flex-col h-full">
  <div class="flex items-center justify-between px-3 py-2 border-b border-neutral-200 bg-white">
    <div role="tablist" class="flex gap-1">
      <button
        type="button"
        role="tab"
        aria-selected={view === 'browse'}
        class="px-3 py-1 text-sm rounded"
        class:bg-blue-50={view === 'browse'}
        class:text-blue-700={view === 'browse'}
        class:font-medium={view === 'browse'}
        class:text-neutral-500={view !== 'browse'}
        onclick={() => (view = 'browse')}
      >
        Browse
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={view === 'installed'}
        class="px-3 py-1 text-sm rounded"
        class:bg-blue-50={view === 'installed'}
        class:text-blue-700={view === 'installed'}
        class:font-medium={view === 'installed'}
        class:text-neutral-500={view !== 'installed'}
        onclick={() => (view = 'installed')}
      >
        Installed
      </button>
    </div>
    <SourcePicker bind:value={source} />
  </div>

  <div class="flex-1 overflow-y-auto relative">
    {#if browseMounted}
      <div class:hidden={view !== 'browse'}>
        <ModBrowseView {source} {instanceId} {mcVersion} {loader} />
      </div>
    {/if}
    {#if installedMounted}
      <div class:hidden={view !== 'installed'}>
        <InstalledModsView {instanceId} {mcVersion} {loader} />
      </div>
    {/if}
  </div>
</div>

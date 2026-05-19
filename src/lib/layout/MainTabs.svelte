<script lang="ts">
  import type { Snippet } from 'svelte';
  import ModBrowserTab from '$lib/mods/ModBrowserTab.svelte';
  import { modBrowserNav } from '$lib/settings/state.svelte';

  type Tab = 'overview' | 'mod_browser' | 'modpacks';

  let {
    overview,
    instanceId = null,
    mcVersion = null,
    loader = null,
  }: {
    overview?: Snippet;
    instanceId?: string | null;
    mcVersion?: string | null;
    loader?: 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;
  } = $props();

  let active = $state<Tab>('overview');

  // Honour the cross-component navigation rune (set by the Overview
  // "Installed mods" link, possibly by other entry points later).
  // ModBrowserTab reads the same rune to pick the sub-view, then
  // clears it.
  $effect(() => {
    if (modBrowserNav.value !== null) {
      active = 'mod_browser';
    }
  });
</script>

<div class="flex flex-col overflow-hidden">
  <div role="tablist" class="border-b border-neutral-200 px-3 flex gap-1 bg-white">
    <button
      type="button"
      role="tab"
      aria-selected={active === 'overview'}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-blue-600={active === 'overview'}
      class:text-neutral-900={active === 'overview'}
      class:font-semibold={active === 'overview'}
      class:border-transparent={active !== 'overview'}
      class:text-neutral-400={active !== 'overview'}
      onclick={() => (active = 'overview')}
    >
      Overview
    </button>
    <button
      type="button"
      role="tab"
      aria-selected={active === 'mod_browser'}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-blue-600={active === 'mod_browser'}
      class:text-neutral-900={active === 'mod_browser'}
      class:font-semibold={active === 'mod_browser'}
      class:border-transparent={active !== 'mod_browser'}
      class:text-neutral-400={active !== 'mod_browser'}
      onclick={() => (active = 'mod_browser')}
    >
      Mod browser
    </button>
    <button
      type="button"
      role="tab"
      aria-selected={active === 'modpacks'}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-blue-600={active === 'modpacks'}
      class:text-neutral-900={active === 'modpacks'}
      class:font-semibold={active === 'modpacks'}
      class:border-transparent={active !== 'modpacks'}
      class:text-neutral-400={active !== 'modpacks'}
      onclick={() => (active = 'modpacks')}
    >
      Modpacks
      <span class="ml-1 text-[10px] bg-neutral-100 text-neutral-500 px-1.5 py-0.5 rounded-full">
        soon
      </span>
    </button>
  </div>

  <div class="flex-1 overflow-y-auto">
    {#if active === 'overview'}
      {#if overview}
        {@render overview()}
      {/if}
    {:else if active === 'mod_browser'}
      <ModBrowserTab {instanceId} {mcVersion} {loader} />
    {:else if active === 'modpacks'}
      <div class="p-8 text-center text-neutral-400 text-sm">
        Coming in v0.5.0 modpack import slice.
      </div>
    {/if}
  </div>
</div>

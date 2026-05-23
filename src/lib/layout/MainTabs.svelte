<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import { onMount } from 'svelte';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import ModBrowserTab from '$lib/mods/ModBrowserTab.svelte';
  import ModpacksTab from '$lib/modpacks/ModpacksTab.svelte';
  import { canInstallMods } from '$lib/mods/install-eligibility';
  import {
    modBrowserNav,
    modpacksNav,
    droppedMods,
    droppedModpack,
    dragActive,
  } from '$lib/settings/state.svelte';

  type Tab = 'overview' | 'mod_browser' | 'modpacks';

  let {
    overview,
    instanceId = null,
    mcVersion = null,
    loader = null,
    instances = [],
    onSwitchInstance = () => {},
    onListChanged = () => {},
  }: {
    overview?: Snippet;
    instanceId?: string | null;
    mcVersion?: string | null;
    loader?: 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;
    instances?: InstanceWithStatus[];
    onSwitchInstance?: (id: string) => void;
    onListChanged?: () => void;
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

  // Honour the Modpacks-tab navigation rune (set by the Overview
  // missing-mods indicator). ModpacksTab + ImportedView read the same
  // rune to open the right drawer; ImportedView clears it.
  $effect(() => {
    if (modpacksNav.value !== null) {
      active = 'modpacks';
    }
  });

  // Whether the active instance can take mods (selected + non-vanilla).
  // Shared with ModBrowserTab via canInstallMods() so the rule lives once.
  const canInstall = $derived(canInstallMods(instanceId, loader));

  // One window-level drag-drop listener for the whole app. Routes a drop
  // by the active top-level tab: .jar → the Mods tab, .mrpack/.zip → the
  // Modpacks tab, nothing on Overview.
  onMount(() => {
    const pending = getCurrentWebview().onDragDropEvent((event) => {
      const t = (event as { payload: { type: string; paths?: string[] } }).payload.type;
      if (active === 'overview') {
        dragActive.value = false;
        return;
      }
      if (t === 'enter' || t === 'over') {
        dragActive.value = true;
      } else if (t === 'leave') {
        dragActive.value = false;
      } else if (t === 'drop') {
        dragActive.value = false;
        const paths =
          (event as { payload: { type: string; paths?: string[] } }).payload.paths ?? [];
        if (active === 'mod_browser') {
          const jars = paths.filter((p) => p.toLowerCase().endsWith('.jar'));
          if (jars.length > 0 && canInstall) {
            droppedMods.value = jars;
          }
        } else if (active === 'modpacks') {
          const pack = paths.find((p) => /\.(mrpack|zip)$/i.test(p));
          if (pack) droppedModpack.value = pack;
        }
      }
    });
    return () => {
      void pending.then((un) => un());
    };
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
      data-tour="tab-mods"
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
      data-tour="tab-modpacks"
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
    </button>
  </div>

  <div class="flex-1 overflow-y-auto relative">
    {#if active === 'overview'}
      {#if overview}
        {@render overview()}
      {/if}
    {:else if active === 'mod_browser'}
      <ModBrowserTab {instanceId} {mcVersion} {loader} />
    {:else if active === 'modpacks'}
      <ModpacksTab
        {instances}
        onInstanceCreated={(id) => {
          onSwitchInstance(id);
          // Open instance / post-import — user expects to land on the
          // active instance's home view, not stay on the (now stale)
          // Modpacks tab.
          active = 'overview';
        }}
        {onListChanged}
      />
    {/if}
  </div>
</div>

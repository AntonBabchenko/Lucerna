<script lang="ts">
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import ModBrowserTab from '$lib/mods/ModBrowserTab.svelte';
  import { canInstallMods } from '$lib/mods/install-eligibility';
  import WorldsTab from '$lib/worlds/WorldsTab.svelte';
  import { modBrowserNav, droppedMods, dragActive } from '$lib/settings/state.svelte';
  import { t } from '$lib/i18n';

  type Tab = 'overview' | 'mod_browser' | 'worlds';

  let {
    overview,
    instanceId = null,
    instanceName = null,
    mcVersion = null,
    loader = null,
    onListChanged = () => {},
  }: {
    overview?: Snippet;
    instanceId?: string | null;
    instanceName?: string | null;
    mcVersion?: string | null;
    loader?: 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;
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

  // Whether the active instance can take mods (selected + non-vanilla).
  // Shared with ModBrowserTab via canInstallMods() so the rule lives once.
  const canInstall = $derived(canInstallMods(instanceId, loader));

  // One window-level drag-drop listener for the per-instance tabs.
  // Modpacks live outside MainTabs now (sidebar-level Browse modpacks
  // view owns its own drag-drop), so this listener only handles .jar
  // drops onto the Mod browser tab.
  onMount(() => {
    const pending = getCurrentWebview().onDragDropEvent((event) => {
      const t = (event as { payload: { type: string; paths?: string[] } }).payload.type;
      if (active !== 'mod_browser') {
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
        const jars = paths.filter((p) => p.toLowerCase().endsWith('.jar'));
        if (jars.length > 0 && canInstall) {
          droppedMods.value = jars;
        }
      }
    });
    return () => {
      void pending.then((un) => un());
    };
  });
</script>

<div class="flex flex-col overflow-hidden">
  <div role="tablist" class="border-b border-border-subtle px-3 flex gap-1 bg-surface">
    <button
      type="button"
      role="tab"
      aria-selected={active === 'overview'}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-accent={active === 'overview'}
      class:text-primary={active === 'overview'}
      class:font-semibold={active === 'overview'}
      class:border-transparent={active !== 'overview'}
      class:text-placeholder={active !== 'overview'}
      onclick={() => (active = 'overview')}
    >
      {$t('nav.overview')}
    </button>
    <button
      type="button"
      role="tab"
      data-tour="tab-mods"
      aria-selected={active === 'mod_browser'}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-accent={active === 'mod_browser'}
      class:text-primary={active === 'mod_browser'}
      class:font-semibold={active === 'mod_browser'}
      class:border-transparent={active !== 'mod_browser'}
      class:text-placeholder={active !== 'mod_browser'}
      onclick={() => (active = 'mod_browser')}
    >
      {$t('nav.modBrowser')}
    </button>
    <button
      type="button"
      role="tab"
      aria-selected={active === 'worlds'}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-accent={active === 'worlds'}
      class:text-primary={active === 'worlds'}
      class:font-semibold={active === 'worlds'}
      class:border-transparent={active !== 'worlds'}
      class:text-placeholder={active !== 'worlds'}
      onclick={() => (active = 'worlds')}
    >
      {$t('nav.worlds')}
    </button>
  </div>

  <div class="flex-1 overflow-y-auto relative">
    {#if active === 'overview'}
      {#if overview}
        {@render overview()}
      {/if}
    {:else if active === 'mod_browser'}
      <ModBrowserTab {instanceId} {instanceName} {mcVersion} {loader} />
    {:else if active === 'worlds'}
      <WorldsTab {instanceId} {onListChanged} />
    {/if}
  </div>
</div>

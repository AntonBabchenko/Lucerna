<script lang="ts">
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import AddonsTab from '$lib/mods/AddonsTab.svelte';
  import { canInstallMods } from '$lib/mods/install-eligibility';
  import WorldsTab from '$lib/worlds/WorldsTab.svelte';
  import {
    modBrowserNav,
    droppedMods,
    droppedAssets,
    addonsKind,
    dragActive,
  } from '$lib/settings/state.svelte';
  import { t } from '$lib/i18n';

  type Tab = 'overview' | 'mod_browser' | 'worlds';

  let {
    overview,
    instanceId = null,
    instanceName = null,
    mcVersion = null,
    loader = null,
    onListChanged = () => {},
    onQuickPlayWorld = () => {},
    quickPlayDisabledReason = null,
  }: {
    overview?: Snippet;
    instanceId?: string | null;
    instanceName?: string | null;
    mcVersion?: string | null;
    loader?: 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;
    onListChanged?: () => void;
    onQuickPlayWorld?: (folderName: string) => void;
    quickPlayDisabledReason?: string | null;
  } = $props();

  let active = $state<Tab>('overview');

  // Tab order for roving-tabindex arrow-key navigation (WAI-ARIA tabs
  // pattern). Kept in render order so ArrowLeft/Right map to visual order.
  const TAB_ORDER: Tab[] = ['overview', 'mod_browser', 'worlds'];
  let tabEls = $state<(HTMLButtonElement | null)[]>([]);

  function onTablistKeydown(e: KeyboardEvent) {
    const current = TAB_ORDER.indexOf(active);
    if (current === -1) return;
    let next = current;
    if (e.key === 'ArrowRight') next = (current + 1) % TAB_ORDER.length;
    else if (e.key === 'ArrowLeft') next = (current - 1 + TAB_ORDER.length) % TAB_ORDER.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = TAB_ORDER.length - 1;
    else return;
    e.preventDefault();
    active = TAB_ORDER[next];
    tabEls[next]?.focus();
  }

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
  // view owns its own drag-drop), so on the Add-ons tab this listener
  // routes `.jar` drops to the Mods segment (droppedMods) and `.zip`
  // drops to the Resource-pack/Shader segments (droppedAssets), keyed
  // off the active `addonsKind`.
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
        if (addonsKind.value === 'mod') {
          const jars = paths.filter((p) => p.toLowerCase().endsWith('.jar'));
          if (jars.length > 0 && canInstall) {
            droppedMods.value = jars;
          }
        } else {
          // Resource-pack / shader segment — accept .zip. Any selected instance
          // qualifies (resource packs run on vanilla; a shader pack file installs
          // regardless of loader).
          const zips = paths.filter((p) => p.toLowerCase().endsWith('.zip'));
          if (zips.length > 0 && instanceId !== null) {
            droppedAssets.value = { kind: addonsKind.value, paths: zips };
          }
        }
      }
    });
    return () => {
      void pending.then((un) => un());
    };
  });
</script>

<div class="flex flex-col overflow-hidden">
  <!-- The tablist is a container; the roving-tabindex tabs inside hold focus,
       so the list itself takes no tabindex. The keydown handler only routes
       arrow keys to those focusable children. -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    role="tablist"
    class="border-b border-border-subtle px-3 flex gap-1 bg-surface"
    onkeydown={onTablistKeydown}
  >
    <button
      bind:this={tabEls[0]}
      type="button"
      role="tab"
      aria-selected={active === 'overview'}
      tabindex={active === 'overview' ? 0 : -1}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-accent={active === 'overview'}
      class:text-primary={active === 'overview'}
      class:font-semibold={active === 'overview'}
      class:border-transparent={active !== 'overview'}
      class:text-muted={active !== 'overview'}
      onclick={() => (active = 'overview')}
    >
      {$t('nav.overview')}
    </button>
    <button
      bind:this={tabEls[1]}
      type="button"
      role="tab"
      data-tour="tab-mods"
      aria-selected={active === 'mod_browser'}
      tabindex={active === 'mod_browser' ? 0 : -1}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-accent={active === 'mod_browser'}
      class:text-primary={active === 'mod_browser'}
      class:font-semibold={active === 'mod_browser'}
      class:border-transparent={active !== 'mod_browser'}
      class:text-muted={active !== 'mod_browser'}
      onclick={() => (active = 'mod_browser')}
    >
      {$t('nav.addons')}
    </button>
    <button
      bind:this={tabEls[2]}
      type="button"
      role="tab"
      aria-selected={active === 'worlds'}
      tabindex={active === 'worlds' ? 0 : -1}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-accent={active === 'worlds'}
      class:text-primary={active === 'worlds'}
      class:font-semibold={active === 'worlds'}
      class:border-transparent={active !== 'worlds'}
      class:text-muted={active !== 'worlds'}
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
      <AddonsTab {instanceId} {instanceName} {mcVersion} {loader} />
    {:else if active === 'worlds'}
      <WorldsTab {instanceId} {onListChanged} {onQuickPlayWorld} {quickPlayDisabledReason} />
    {/if}
  </div>
</div>

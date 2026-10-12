<script lang="ts">
  import { untrack, type Snippet } from 'svelte';
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import AddonsTab from '$lib/mods/AddonsTab.svelte';
  import WorldsTab from '$lib/worlds/WorldsTab.svelte';
  import ScreenshotsTab from '$lib/screenshots/ScreenshotsTab.svelte';
  import { modBrowserNav, clientActiveTab } from '$lib/settings/state.svelte';
  import { t } from '$lib/i18n';
  import {
    clearInstanceSettingsNav,
    instanceSettingsNav,
    requestIsFor,
  } from '$lib/instances/settings/instance-settings-nav.svelte';

  type Tab = 'overview' | 'mod_browser' | 'worlds' | 'screenshots' | 'settings';

  let {
    overview,
    settings,
    instanceId = null,
    instanceName = null,
    mcVersion = null,
    loader = null,
    loaderVersion = null,
    instances = [],
    onListChanged = () => {},
    onWorldsChanged = () => {},
    onQuickPlayWorld = () => {},
    quickPlayDisabledReason = null,
    running = false,
    hasInstalledMods = false,
    onManageLoader = () => {},
  }: {
    overview?: Snippet;
    /** The active instance's settings (the page renders InstanceSettingsTab, which
     *  needs data and handlers MainTabs has no business carrying). */
    settings?: Snippet;
    instanceId?: string | null;
    instanceName?: string | null;
    mcVersion?: string | null;
    loader?: 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;
    loaderVersion?: string | null;
    /** Every instance — the Worlds tab's migrate dialog picks its target from it. */
    instances?: InstanceWithStatus[];
    onListChanged?: () => void;
    /** The Worlds tab's world set changed on disk (a migration landed). */
    onWorldsChanged?: () => void;
    onQuickPlayWorld?: (folderName: string) => void;
    quickPlayDisabledReason?: string | null;
    running?: boolean;
    /** Forwarded to AddonsTab (sub-tab default, D10); null = not counted yet. */
    hasInstalledMods?: boolean | null;
    /** Opens Manage profiles on the loader field (the Shaders hint's «Choose a mod loader»). */
    onManageLoader?: () => void;
  } = $props();

  // A pending request for the Settings tab (instance-settings-nav) decides where
  // a freshly mounted MainTabs starts: mounting on Overview only to leave it
  // would run Overview's first-visit tour for a frame and mark it seen. Also
  // one still waiting on its activation (compact mode expands for it): the tab
  // shows the active instance until the activation lands, and a failed one
  // leaves the user on the tab they asked for.
  let active = $state<Tab>(
    untrack(() => instanceSettingsNav.value !== null) ? 'settings' : 'overview',
  );

  // Tab order for roving-tabindex arrow-key navigation (WAI-ARIA tabs
  // pattern). Kept in render order so ArrowLeft/Right map to visual order.
  const TAB_ORDER: Tab[] = ['overview', 'mod_browser', 'worlds', 'screenshots', 'settings'];

  // Every user choice of a tab goes through here. Choosing another tab drops a
  // Settings request still waiting on an activation: it would otherwise pull the
  // user back to Settings, unasked, once that activation lands.
  function selectTab(tab: Tab) {
    if (tab !== 'settings') clearInstanceSettingsNav();
    active = tab;
  }
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
    selectTab(TAB_ORDER[next]);
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

  // A request for the Settings tab (an Overview card, the sidebar, Rename) is
  // delivered once it is about the active instance; the tab itself takes the
  // field it asks for and clears the request.
  $effect(() => {
    if (requestIsFor(instanceSettingsNav.value, instanceId)) active = 'settings';
  });

  // Mirror the active tab for the window-level drop router in +page.svelte
  // (it routes drops by whether the client sits on Add-ons or Worlds), and take
  // it back when MainTabs goes: compact mode unmounts the whole content column,
  // and a mirror left at 'mod_browser' routed a jar dropped on the compact
  // window into a list nobody consumed — it installed on the next visit to
  // Add-ons, unannounced. With no tab showing, the drop is refused and says so.
  $effect(() => {
    clientActiveTab.value = active;
    return () => {
      clientActiveTab.value = null;
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
      id="maintab-overview"
      aria-controls="maintabpanel"
      aria-selected={active === 'overview'}
      tabindex={active === 'overview' ? 0 : -1}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-accent={active === 'overview'}
      class:text-primary={active === 'overview'}
      class:font-semibold={active === 'overview'}
      class:border-transparent={active !== 'overview'}
      class:text-muted={active !== 'overview'}
      onclick={() => selectTab('overview')}
    >
      {$t('nav.overview')}
    </button>
    <button
      bind:this={tabEls[1]}
      type="button"
      role="tab"
      id="maintab-mod_browser"
      aria-controls="maintabpanel"
      data-tour="tab-mods"
      aria-selected={active === 'mod_browser'}
      tabindex={active === 'mod_browser' ? 0 : -1}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-accent={active === 'mod_browser'}
      class:text-primary={active === 'mod_browser'}
      class:font-semibold={active === 'mod_browser'}
      class:border-transparent={active !== 'mod_browser'}
      class:text-muted={active !== 'mod_browser'}
      onclick={() => selectTab('mod_browser')}
    >
      {$t('nav.addons')}
    </button>
    <button
      bind:this={tabEls[2]}
      type="button"
      role="tab"
      id="maintab-worlds"
      aria-controls="maintabpanel"
      aria-selected={active === 'worlds'}
      tabindex={active === 'worlds' ? 0 : -1}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-accent={active === 'worlds'}
      class:text-primary={active === 'worlds'}
      class:font-semibold={active === 'worlds'}
      class:border-transparent={active !== 'worlds'}
      class:text-muted={active !== 'worlds'}
      onclick={() => selectTab('worlds')}
    >
      {$t('nav.worlds')}
    </button>
    <button
      bind:this={tabEls[3]}
      type="button"
      role="tab"
      id="maintab-screenshots"
      aria-controls="maintabpanel"
      aria-selected={active === 'screenshots'}
      tabindex={active === 'screenshots' ? 0 : -1}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-accent={active === 'screenshots'}
      class:text-primary={active === 'screenshots'}
      class:font-semibold={active === 'screenshots'}
      class:border-transparent={active !== 'screenshots'}
      class:text-muted={active !== 'screenshots'}
      onclick={() => selectTab('screenshots')}
    >
      {$t('nav.screenshots')}
    </button>
    <button
      bind:this={tabEls[4]}
      type="button"
      role="tab"
      id="maintab-settings"
      aria-controls="maintabpanel"
      aria-label={$t('instance.settings.tabAria')}
      data-tour="tab-settings"
      aria-selected={active === 'settings'}
      tabindex={active === 'settings' ? 0 : -1}
      class="px-3 py-2 text-base border-b-2 -mb-px"
      class:border-accent={active === 'settings'}
      class:text-primary={active === 'settings'}
      class:font-semibold={active === 'settings'}
      class:border-transparent={active !== 'settings'}
      class:text-muted={active !== 'settings'}
      onclick={() => selectTab('settings')}
    >
      {$t('nav.settings')}
    </button>
  </div>

  <!-- The Settings tab owns its scroll container (like Add-ons owns its list's),
       so the panel does not scroll a second time around it. -->
  <div
    class="flex-1 relative {active === 'settings' ? 'overflow-hidden' : 'overflow-y-auto'}"
    role="tabpanel"
    id="maintabpanel"
    aria-labelledby="maintab-{active}"
    tabindex="0"
  >
    {#if active === 'overview'}
      {#if overview}
        {@render overview()}
      {/if}
    {:else if active === 'mod_browser'}
      <AddonsTab
        {instanceId}
        {instanceName}
        {mcVersion}
        {loader}
        {loaderVersion}
        {hasInstalledMods}
        {onManageLoader}
      />
    {:else if active === 'worlds'}
      <WorldsTab
        {instanceId}
        {instanceName}
        {instances}
        {onListChanged}
        {onWorldsChanged}
        {onQuickPlayWorld}
        {quickPlayDisabledReason}
        {running}
      />
    {:else if active === 'screenshots'}
      <ScreenshotsTab {instanceId} />
    {:else if active === 'settings'}
      {#if settings}
        {@render settings()}
      {/if}
    {/if}
  </div>
</div>

<script lang="ts">
  import type { Snippet } from 'svelte';
  import AddonsTab from '$lib/mods/AddonsTab.svelte';
  import WorldsTab from '$lib/worlds/WorldsTab.svelte';
  import ScreenshotsTab from '$lib/screenshots/ScreenshotsTab.svelte';
  import { modBrowserNav, clientActiveTab } from '$lib/settings/state.svelte';
  import { t } from '$lib/i18n';

  type Tab = 'overview' | 'mod_browser' | 'worlds' | 'screenshots';

  let {
    overview,
    instanceId = null,
    instanceName = null,
    mcVersion = null,
    loader = null,
    loaderVersion = null,
    onListChanged = () => {},
    onQuickPlayWorld = () => {},
    quickPlayDisabledReason = null,
    running = false,
  }: {
    overview?: Snippet;
    instanceId?: string | null;
    instanceName?: string | null;
    mcVersion?: string | null;
    loader?: 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;
    loaderVersion?: string | null;
    onListChanged?: () => void;
    onQuickPlayWorld?: (folderName: string) => void;
    quickPlayDisabledReason?: string | null;
    running?: boolean;
  } = $props();

  let active = $state<Tab>('overview');

  // Tab order for roving-tabindex arrow-key navigation (WAI-ARIA tabs
  // pattern). Kept in render order so ArrowLeft/Right map to visual order.
  const TAB_ORDER: Tab[] = ['overview', 'mod_browser', 'worlds', 'screenshots'];
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

  // Mirror the active tab for the window-level drop router in +page.svelte
  // (it routes drops by whether the client sits on Add-ons or Worlds).
  $effect(() => {
    clientActiveTab.value = active;
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
      onclick={() => (active = 'overview')}
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
      onclick={() => (active = 'mod_browser')}
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
      onclick={() => (active = 'worlds')}
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
      onclick={() => (active = 'screenshots')}
    >
      {$t('nav.screenshots')}
    </button>
  </div>

  <div
    class="flex-1 overflow-y-auto relative"
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
      <AddonsTab {instanceId} {instanceName} {mcVersion} {loader} {loaderVersion} />
    {:else if active === 'worlds'}
      <WorldsTab
        {instanceId}
        {onListChanged}
        {onQuickPlayWorld}
        {quickPlayDisabledReason}
        {running}
      />
    {:else if active === 'screenshots'}
      <ScreenshotsTab {instanceId} />
    {/if}
  </div>
</div>

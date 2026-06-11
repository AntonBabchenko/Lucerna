<script lang="ts">
  // Settings modal shell — Task 18 of the v0.5.0 mod browser plan.
  //
  // Reads/writes the shared `settingsOpen` rune from `state.svelte.ts`:
  // when its `.value` becomes non-null, the modal mounts and focuses
  // whichever tab the caller requested ('curseforge' or 'storage').
  // Setting `.value = null` (× button, backdrop click, or Escape)
  // closes the modal.
  //
  // The CurseForge tab mounts CurseForgeKeyForm (Task 19); the Storage
  // tab mounts StoragePanel (Task 20). Both are self-contained — the
  // shell only routes between them.
  import AboutPanel from './AboutPanel.svelte';
  import CurseForgeKeyForm from './CurseForgeKeyForm.svelte';
  import GeneralPanel from './GeneralPanel.svelte';
  import StoragePanel from './StoragePanel.svelte';
  import ChangelogPanel from '$lib/changelog/ChangelogPanel.svelte';
  import { CHANGELOG } from '$lib/changelog/source';
  import { settingsOpen, type SettingsTab } from './state.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';

  let active = $state<SettingsTab>('general');

  // Tab order for roving-tabindex arrow-key navigation (WAI-ARIA tabs
  // pattern), in render order.
  const TAB_ORDER: SettingsTab[] = ['general', 'changelog', 'storage', 'curseforge', 'about'];
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

  // When something opens the modal at a specific tab, snap to it. The
  // untracked rune dependency is fine — every $effect cycle re-reads
  // `settingsOpen.value`, which is the trigger we want.
  $effect(() => {
    if (settingsOpen.value?.tab) {
      active = settingsOpen.value.tab;
    }
  });

  function close() {
    settingsOpen.value = null;
  }
</script>

{#if settingsOpen.value}
  <Modal
    ariaLabelledby="settings-title"
    onClose={close}
    panelClass="w-[640px] max-w-[95vw] h-[min(80vh,600px)] flex flex-col"
  >
    <header class="flex items-center justify-between px-4 py-3 border-b shrink-0">
      <h2 id="settings-title" class="text-base font-semibold text-primary">
        {$t('settings.title')}
      </h2>
      <CloseButton onClick={close} ariaLabel={$t('settings.closeLabel')} />
    </header>
    <!-- The tablist is a container; the roving-tabindex tabs inside hold
           focus, so the list itself takes no tabindex. The keydown handler only
           routes arrow keys to those focusable children. -->
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div role="tablist" class="px-4 pt-2 border-b flex gap-1 shrink-0" onkeydown={onTablistKeydown}>
      <button
        bind:this={tabEls[0]}
        type="button"
        role="tab"
        aria-selected={active === 'general'}
        tabindex={active === 'general' ? 0 : -1}
        class="px-3 py-1 text-sm border-b-2 -mb-px"
        class:border-accent={active === 'general'}
        class:text-primary={active === 'general'}
        class:font-medium={active === 'general'}
        class:border-transparent={active !== 'general'}
        class:text-muted={active !== 'general'}
        onclick={() => (active = 'general')}
      >
        {$t('settings.tabs.general')}
      </button>
      <button
        bind:this={tabEls[1]}
        type="button"
        role="tab"
        aria-selected={active === 'changelog'}
        tabindex={active === 'changelog' ? 0 : -1}
        class="px-3 py-1 text-sm border-b-2 -mb-px"
        class:border-accent={active === 'changelog'}
        class:text-primary={active === 'changelog'}
        class:font-medium={active === 'changelog'}
        class:border-transparent={active !== 'changelog'}
        class:text-muted={active !== 'changelog'}
        onclick={() => (active = 'changelog')}
      >
        {$t('settings.tabs.changelog')}
      </button>
      <button
        bind:this={tabEls[2]}
        type="button"
        role="tab"
        aria-selected={active === 'storage'}
        tabindex={active === 'storage' ? 0 : -1}
        class="px-3 py-1 text-sm border-b-2 -mb-px"
        class:border-accent={active === 'storage'}
        class:text-primary={active === 'storage'}
        class:font-medium={active === 'storage'}
        class:border-transparent={active !== 'storage'}
        class:text-muted={active !== 'storage'}
        onclick={() => (active = 'storage')}
      >
        {$t('settings.tabs.storage')}
      </button>
      <button
        bind:this={tabEls[3]}
        type="button"
        role="tab"
        aria-selected={active === 'curseforge'}
        tabindex={active === 'curseforge' ? 0 : -1}
        class="px-3 py-1 text-sm border-b-2 -mb-px"
        class:border-accent={active === 'curseforge'}
        class:text-primary={active === 'curseforge'}
        class:font-medium={active === 'curseforge'}
        class:border-transparent={active !== 'curseforge'}
        class:text-muted={active !== 'curseforge'}
        onclick={() => (active = 'curseforge')}
      >
        {$t('settings.tabs.curseforge')}
      </button>
      <button
        bind:this={tabEls[4]}
        type="button"
        role="tab"
        aria-selected={active === 'about'}
        tabindex={active === 'about' ? 0 : -1}
        class="px-3 py-1 text-sm border-b-2 -mb-px"
        class:border-accent={active === 'about'}
        class:text-primary={active === 'about'}
        class:font-medium={active === 'about'}
        class:border-transparent={active !== 'about'}
        class:text-muted={active !== 'about'}
        onclick={() => (active = 'about')}
      >
        {$t('settings.tabs.about')}
      </button>
    </div>
    <div class="p-4 overflow-y-auto flex-1">
      {#if active === 'general'}
        <GeneralPanel />
      {:else if active === 'changelog'}
        <ChangelogPanel entries={CHANGELOG} />
      {:else if active === 'storage'}
        <StoragePanel />
      {:else if active === 'curseforge'}
        <CurseForgeKeyForm />
      {:else}
        <AboutPanel />
      {/if}
    </div>
  </Modal>
{/if}

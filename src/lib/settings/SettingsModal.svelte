<script lang="ts">
  // Settings modal shell. A widened modal with a vertical sidebar list of
  // 7 sections (left) and the active section's panel (right). The sidebar
  // is a vertical WAI-ARIA tablist with roving tabindex (ArrowUp/Down +
  // Home/End). Deep-links arrive via the shared `settingsOpen` rune:
  // `settingsOpen.value = { tab }` snaps to that section.
  import AppearancePanel from './AppearancePanel.svelte';
  import GamePanel from './GamePanel.svelte';
  import CurseForgeKeyForm from './CurseForgeKeyForm.svelte';
  import UrlSchemeSection from './UrlSchemeSection.svelte';
  import StoragePanel from './StoragePanel.svelte';
  import UpdatesPanel from './UpdatesPanel.svelte';
  import HelpPanel from './HelpPanel.svelte';
  import AboutPanel from './AboutPanel.svelte';
  import { settingsOpen, type SettingsTab } from './state.svelte';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';

  const SECTIONS: { id: SettingsTab; labelKey: TranslationKey }[] = [
    { id: 'appearance', labelKey: 'settings.sections.appearance' },
    { id: 'game', labelKey: 'settings.sections.game' },
    { id: 'integrations', labelKey: 'settings.sections.integrations' },
    { id: 'storage', labelKey: 'settings.sections.storage' },
    { id: 'updates', labelKey: 'settings.sections.updates' },
    { id: 'help', labelKey: 'settings.sections.help' },
    { id: 'about', labelKey: 'settings.sections.about' },
  ];

  let active = $state<SettingsTab>('appearance');
  let tabEls = $state<(HTMLButtonElement | null)[]>([]);

  function onTablistKeydown(e: KeyboardEvent) {
    const current = SECTIONS.findIndex((s) => s.id === active);
    if (current === -1) return;
    let next = current;
    if (e.key === 'ArrowDown') next = (current + 1) % SECTIONS.length;
    else if (e.key === 'ArrowUp') next = (current - 1 + SECTIONS.length) % SECTIONS.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = SECTIONS.length - 1;
    else return;
    e.preventDefault();
    active = SECTIONS[next].id;
    tabEls[next]?.focus();
  }

  // When something opens the modal at a specific section, snap to it.
  $effect(() => {
    if (settingsOpen.value?.tab) active = settingsOpen.value.tab;
  });

  function close() {
    settingsOpen.value = null;
  }
</script>

{#if settingsOpen.value}
  <Modal
    ariaLabelledby="settings-title"
    onClose={close}
    panelClass="w-[720px] max-w-[95vw] h-[min(80vh,600px)] flex flex-col"
  >
    <header class="flex items-center justify-between px-4 py-3 border-b shrink-0">
      <h2 id="settings-title" class="text-base font-semibold text-primary">
        {$t('settings.title')}
      </h2>
      <CloseButton onClick={close} ariaLabel={$t('settings.closeLabel')} />
    </header>
    <div class="flex flex-1 min-h-0">
      <!-- Vertical tablist: the roving-tabindex tabs inside hold focus, so
             the list takes no tabindex; the keydown handler routes arrows. -->
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        role="tablist"
        aria-orientation="vertical"
        class="w-44 shrink-0 border-r p-2 flex flex-col gap-0.5 overflow-y-auto"
        onkeydown={onTablistKeydown}
      >
        {#each SECTIONS as s, i (s.id)}
          <button
            bind:this={tabEls[i]}
            type="button"
            role="tab"
            aria-selected={active === s.id}
            tabindex={active === s.id ? 0 : -1}
            class="text-left px-3 py-1.5 text-sm rounded border-l-2"
            class:border-accent={active === s.id}
            class:text-primary={active === s.id}
            class:font-medium={active === s.id}
            class:border-transparent={active !== s.id}
            class:text-muted={active !== s.id}
            onclick={() => (active = s.id)}
          >
            {$t(s.labelKey)}
          </button>
        {/each}
      </div>
      <div class="flex-1 overflow-y-auto p-4">
        {#if active === 'appearance'}
          <AppearancePanel />
        {:else if active === 'game'}
          <GamePanel />
        {:else if active === 'integrations'}
          <div class="flex flex-col gap-6">
            <CurseForgeKeyForm />
            <div class="border-t pt-4">
              <UrlSchemeSection />
            </div>
          </div>
        {:else if active === 'storage'}
          <StoragePanel />
        {:else if active === 'updates'}
          <UpdatesPanel />
        {:else if active === 'help'}
          <HelpPanel />
        {:else}
          <AboutPanel />
        {/if}
      </div>
    </div>
  </Modal>
{/if}

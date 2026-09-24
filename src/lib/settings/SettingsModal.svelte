<script lang="ts">
  // Settings modal shell. A widened modal with a vertical sidebar list of
  // 7 sections (left) and the active section's panel (right). The sidebar
  // is a vertical WAI-ARIA tablist with roving tabindex (ArrowUp/Down +
  // Home/End). Deep-links arrive via the shared `settingsOpen` rune:
  // `settingsOpen.value = { tab }` snaps to that section.
  import { tick, untrack } from 'svelte';
  import AppearancePanel from './AppearancePanel.svelte';
  import GamePanel from './GamePanel.svelte';
  import CurseForgeKeyForm from './CurseForgeKeyForm.svelte';
  import AiTranslationSection from './AiTranslationSection.svelte';
  import StoragePanel from './StoragePanel.svelte';
  import PrivacyPanel from './PrivacyPanel.svelte';
  import UpdatesPanel from './UpdatesPanel.svelte';
  import HelpPanel from './HelpPanel.svelte';
  import AboutPanel from './AboutPanel.svelte';
  import SettingsSearchField from './SettingsSearchField.svelte';
  import SettingsField from './SettingsField.svelte';
  import { dataLocation } from './data-location.svelte';
  import {
    closeSettings,
    settingsJumpFocus,
    settingsOpen,
    settingsSearchFocus,
    type SettingsTab,
  } from './state.svelte';
  import { SETTINGS_SEARCH, type SettingsSearchEntry } from './search-index';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';

  const SECTIONS: { id: SettingsTab; labelKey: TranslationKey }[] = [
    { id: 'appearance', labelKey: 'settings.sections.appearance' },
    { id: 'game', labelKey: 'settings.sections.game' },
    { id: 'integrations', labelKey: 'settings.sections.integrations' },
    { id: 'privacy', labelKey: 'settings.sections.privacy' },
    { id: 'storage', labelKey: 'settings.sections.storage' },
    { id: 'updates', labelKey: 'settings.sections.updates' },
    { id: 'help', labelKey: 'settings.sections.help' },
    { id: 'about', labelKey: 'settings.sections.about' },
  ];

  let active = $state<SettingsTab>('appearance');
  let tabEls = $state<(HTMLButtonElement | null)[]>([]);
  let searching = $state(false);
  let announce = $state('');

  // Jump to a searched setting: clear any prior flash, switch to the owning
  // section, wait for that panel to mount, then point the rune at the anchor so
  // its SettingsField flashes. The null→anchor edge re-fires even when the same
  // result is picked twice.
  async function selectResult(entry: SettingsSearchEntry) {
    settingsSearchFocus.value = null;
    active = entry.tab;
    await tick();
    settingsSearchFocus.value = entry.anchor;
    announce = `${$t('settings.search.jumpedTo')} ${$t(entry.labelKey)}, ${$t(`settings.sections.${entry.tab}` as TranslationKey)}`;
  }

  // A jump asked for from inside the modal (jumpInSettings) takes the same
  // path; its rune stays set until the target field has taken focus.
  $effect(() => {
    const anchor = settingsJumpFocus.value;
    if (anchor === null) return;
    untrack(() => void selectResult(SETTINGS_SEARCH[anchor]));
  });

  // A tab change by the user drops a jump that was never delivered (its tab
  // never mounted): the user has moved on. selectResult and openSettingsAt
  // set the rune only AFTER their own tab switch, so this never eats theirs.
  function selectTab(id: SettingsTab) {
    settingsSearchFocus.value = null;
    settingsJumpFocus.value = null;
    active = id;
  }

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
    selectTab(SECTIONS[next].id);
    tabEls[next]?.focus();
  }

  // When something opens the modal at a specific section, snap to it.
  $effect(() => {
    if (settingsOpen.value?.tab) active = settingsOpen.value.tab;
  });

  function close() {
    closeSettings();
    searching = false;
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
    <div class="sr-only" role="status" aria-live="polite">{announce}</div>
    {#if dataLocation.fellBack}
      <!-- ONE notice for the whole modal: in a recovery session preference saves are accepted
           for the session (refusing them would freeze the UI language and the theme, because
           every settings store rolls back on a failed write) and land in a throwaway root. Said
           about the DATA FOLDER, not "nothing is kept": keyring-backed keys do persist. -->
      <div
        class="shrink-0 border-b bg-warning-bg px-4 py-2 text-sm text-warning-text"
        role="status"
        data-testid="settings-recovery-notice"
      >
        {$t('settings.fallbackNotice')}
      </div>
    {/if}
    <div class="flex flex-1 min-h-0">
      <div class="w-44 shrink-0 border-r flex flex-col min-h-0">
        <SettingsSearchField bind:searching onselect={selectResult} />
        {#if !searching}
          <!-- Vertical tablist: the roving-tabindex tabs inside hold focus, so
                 the list takes no tabindex; the keydown handler routes arrows. -->
          <!-- svelte-ignore a11y_interactive_supports_focus -->
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div
            role="tablist"
            aria-orientation="vertical"
            class="p-2 flex flex-col gap-0.5 overflow-y-auto"
            onkeydown={onTablistKeydown}
          >
            {#each SECTIONS as s, i (s.id)}
              <button
                bind:this={tabEls[i]}
                type="button"
                role="tab"
                aria-selected={active === s.id}
                tabindex={active === s.id ? 0 : -1}
                class="text-left px-3 py-1.5 text-sm rounded border-l-2 hover:bg-subtle"
                class:border-accent={active === s.id}
                class:text-primary={active === s.id}
                class:font-semibold={active === s.id}
                class:border-transparent={active !== s.id}
                class:text-muted={active !== s.id}
                onclick={() => selectTab(s.id)}
              >
                {$t(s.labelKey)}
              </button>
            {/each}
          </div>
        {/if}
      </div>
      <div class="flex-1 overflow-y-auto p-4">
        {#if active === 'appearance'}
          <AppearancePanel />
        {:else if active === 'game'}
          <GamePanel />
        {:else if active === 'integrations'}
          <div class="flex flex-col gap-6">
            <SettingsField anchor="integrations.curseforgeKey">
              <CurseForgeKeyForm />
            </SettingsField>
            <div class="border-t pt-4">
              <SettingsField anchor="integrations.aiTranslation">
                <AiTranslationSection />
              </SettingsField>
            </div>
          </div>
        {:else if active === 'privacy'}
          <PrivacyPanel />
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

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
  import { settingsOpen, type SettingsTab } from './state.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';

  let active = $state<SettingsTab>('general');

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

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && settingsOpen.value) {
      close();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if settingsOpen.value}
  <div class="fixed inset-0 z-40 flex items-center justify-center bg-black/40">
    <!-- Backdrop button. Clicking outside the dialog dismisses it. -->
    <button type="button" class="absolute inset-0" aria-label="Close Settings" onclick={close}
    ></button>
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
      class="relative bg-surface rounded shadow-xl w-[640px] max-w-[95vw] h-[min(80vh,600px)] flex flex-col"
    >
      <header class="flex items-center justify-between px-4 py-3 border-b shrink-0">
        <h2 class="text-base font-semibold text-primary">Settings</h2>
        <CloseButton onClick={close} ariaLabel="Close settings" />
      </header>
      <div role="tablist" class="px-4 pt-2 border-b flex gap-1 shrink-0">
        <button
          type="button"
          role="tab"
          aria-selected={active === 'general'}
          class="px-3 py-1 text-sm border-b-2 -mb-px"
          class:border-accent={active === 'general'}
          class:text-primary={active === 'general'}
          class:font-medium={active === 'general'}
          class:border-transparent={active !== 'general'}
          class:text-placeholder={active !== 'general'}
          onclick={() => (active = 'general')}
        >
          General
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={active === 'storage'}
          class="px-3 py-1 text-sm border-b-2 -mb-px"
          class:border-accent={active === 'storage'}
          class:text-primary={active === 'storage'}
          class:font-medium={active === 'storage'}
          class:border-transparent={active !== 'storage'}
          class:text-placeholder={active !== 'storage'}
          onclick={() => (active = 'storage')}
        >
          Storage
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={active === 'curseforge'}
          class="px-3 py-1 text-sm border-b-2 -mb-px"
          class:border-accent={active === 'curseforge'}
          class:text-primary={active === 'curseforge'}
          class:font-medium={active === 'curseforge'}
          class:border-transparent={active !== 'curseforge'}
          class:text-placeholder={active !== 'curseforge'}
          onclick={() => (active = 'curseforge')}
        >
          CurseForge
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={active === 'about'}
          class="px-3 py-1 text-sm border-b-2 -mb-px"
          class:border-accent={active === 'about'}
          class:text-primary={active === 'about'}
          class:font-medium={active === 'about'}
          class:border-transparent={active !== 'about'}
          class:text-placeholder={active !== 'about'}
          onclick={() => (active = 'about')}
        >
          About
        </button>
      </div>
      <div class="p-4 overflow-y-auto flex-1">
        {#if active === 'general'}
          <GeneralPanel />
        {:else if active === 'storage'}
          <StoragePanel />
        {:else if active === 'curseforge'}
          <CurseForgeKeyForm />
        {:else}
          <AboutPanel />
        {/if}
      </div>
    </div>
  </div>
{/if}

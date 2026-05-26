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

  let active = $state<SettingsTab>('curseforge');

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
      class="relative bg-white rounded shadow-xl w-[640px] max-w-[95vw] max-h-[80vh] overflow-y-auto"
    >
      <header class="flex items-center justify-between px-4 py-3 border-b">
        <h2 class="text-base font-semibold">Settings</h2>
        <button
          type="button"
          class="text-muted hover:text-primary text-lg leading-none px-1"
          aria-label="Close"
          onclick={close}
        >
          ×
        </button>
      </header>
      <div role="tablist" class="px-4 pt-2 border-b flex gap-1">
        <button
          type="button"
          role="tab"
          aria-selected={active === 'curseforge'}
          class="px-3 py-1 text-sm rounded -mb-px"
          class:font-medium={active === 'curseforge'}
          class:border-b-2={active === 'curseforge'}
          class:border-accent={active === 'curseforge'}
          onclick={() => (active = 'curseforge')}
        >
          CurseForge
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={active === 'storage'}
          class="px-3 py-1 text-sm rounded -mb-px"
          class:font-medium={active === 'storage'}
          class:border-b-2={active === 'storage'}
          class:border-accent={active === 'storage'}
          onclick={() => (active = 'storage')}
        >
          Storage
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={active === 'about'}
          class="px-3 py-1 text-sm rounded -mb-px"
          class:font-medium={active === 'about'}
          class:border-b-2={active === 'about'}
          class:border-accent={active === 'about'}
          onclick={() => (active = 'about')}
        >
          About
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={active === 'general'}
          class="px-3 py-1 text-sm rounded -mb-px"
          class:font-medium={active === 'general'}
          class:border-b-2={active === 'general'}
          class:border-accent={active === 'general'}
          onclick={() => (active = 'general')}
        >
          General
        </button>
      </div>
      <div class="p-4">
        {#if active === 'curseforge'}
          <CurseForgeKeyForm />
        {:else if active === 'storage'}
          <StoragePanel />
        {:else if active === 'about'}
          <AboutPanel />
        {:else}
          <GeneralPanel />
        {/if}
      </div>
    </div>
  </div>
{/if}

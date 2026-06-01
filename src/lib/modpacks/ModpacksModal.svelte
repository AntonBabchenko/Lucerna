<script lang="ts">
  import type { Snippet } from 'svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';

  // Full-viewport modal shell for the modpacks browser. Purely presentational:
  // it owns the scrim, the large centered panel, and the header (title + ← Back
  // + ×), and projects its body via the `children` snippet. The modpacks browser
  // is NOT instance-scoped (installing a pack creates a new instance), so framing
  // it as a modal over a scrim signals "separate context, not the current
  // instance" — the spatial fix for the modpack/mod confusion.
  let {
    open = false,
    importing = false,
    onClose,
    children,
  }: {
    open?: boolean;
    importing?: boolean;
    onClose: () => void;
    children?: Snippet;
  } = $props();

  // Close is suppressed while a modpack import is in flight. ModpacksTab — which
  // owns the import channels, the onmessage closures, and the progress view —
  // lives inside this modal, so unmounting it mid-import would drop the progress
  // UI and the onInstanceCreated callback. The parent flips `open` to false on
  // the 'done' phase, so the modal still auto-closes when the import finishes.
  function requestClose() {
    if (importing) return;
    onClose();
  }

  // Baseline a11y matching the project's other dialogs: move focus to the Back
  // control on open. (A reusable focus-trap is tracked project-wide debt and is
  // out of scope here.)
  let backButton = $state<HTMLButtonElement | null>(null);
  $effect(() => {
    if (open) backButton?.focus();
  });
</script>

<svelte:window
  onkeydown={(e) => {
    if (open && e.key === 'Escape') requestClose();
  }}
/>

{#if open}
  <div class="fixed inset-0 z-40 flex items-center justify-center" data-testid="modpacks-modal">
    <button
      type="button"
      class="absolute inset-0 bg-black/40"
      aria-label="Close"
      onclick={requestClose}
    ></button>
    <div
      class="relative bg-surface rounded shadow-lg w-[92vw] max-w-5xl h-[92vh] flex flex-col m-4"
      role="dialog"
      aria-modal="true"
      aria-label="Modpacks"
    >
      <header class="p-4 border-b border-border-subtle flex items-center shrink-0">
        <h2 class="flex-1 font-semibold text-primary">Modpacks</h2>
        <button
          type="button"
          bind:this={backButton}
          class="btn-secondary btn-sm mr-2"
          onclick={requestClose}
          data-testid="modpacks-modal-back"
        >
          ← Back
        </button>
        <CloseButton onClick={requestClose} ariaLabel="Close modpacks" />
      </header>
      <div class="flex-1 overflow-hidden flex flex-col min-h-0">
        {@render children?.()}
      </div>
    </div>
  </div>
{/if}

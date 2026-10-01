<script lang="ts">
  // Test fixture: the Modpacks modal as +page.svelte mounts it (ModpacksModal around ModpacksTab),
  // with an optional plain dialog after it — the way Settings, opened from the modal's
  // CurseForge-key banner, paints on top of it.
  import ModpacksModal from '$lib/modpacks/ModpacksModal.svelte';
  import ModpacksTab from '$lib/modpacks/ModpacksTab.svelte';
  import Modal from '$lib/ui/Modal.svelte';

  let { dialogOnTop = false }: { dialogOnTop?: boolean } = $props();
</script>

<ModpacksModal open onClose={() => {}}>
  <ModpacksTab instances={[]} onInstanceCreated={() => {}} />
</ModpacksModal>
{#if dialogOnTop}
  <Modal onClose={() => {}} ariaLabel="Settings">
    <p>Settings</p>
  </Modal>
{/if}

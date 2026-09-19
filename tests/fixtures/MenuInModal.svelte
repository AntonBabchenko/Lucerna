<script lang="ts">
  import Modal from '$lib/ui/Modal.svelte';
  import ContextMenu from '$lib/ui/cards/ContextMenu.svelte';

  let { onModalClose }: { onModalClose: () => void } = $props();
</script>

<Modal ariaLabel="Host" onClose={onModalClose}>
  <ContextMenu items={[{ label: 'Rename', onSelect: () => {} }]} ariaLabel="Row actions">
    <button type="button" data-testid="row">row</button>
  </ContextMenu>
</Modal>

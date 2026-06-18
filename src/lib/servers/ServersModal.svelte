<script lang="ts">
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import ServersView from './ServersView.svelte';

  let {
    open = false,
    onClose,
    instances,
    versions,
  }: {
    open?: boolean;
    onClose: () => void;
    instances: InstanceWithStatus[];
    versions: VersionEntry[];
  } = $props();
</script>

{#if open}
  <Modal
    ariaLabelledby="servers-modal-title"
    {onClose}
    panelClass="w-[92vw] max-w-5xl h-[92vh] flex flex-col"
  >
    <header
      class="p-4 border-b border-border-subtle flex items-center gap-1 shrink-0"
      data-testid="servers-modal"
    >
      <h2 id="servers-modal-title" class="font-semibold text-primary">
        {$t('servers.title')}
      </h2>
      <span class="flex-1"></span>
      <CloseButton onClick={onClose} ariaLabel={$t('common.close')} />
    </header>
    <div class="flex-1 overflow-hidden flex flex-col min-h-0">
      <ServersView {instances} {versions} />
    </div>
  </Modal>
{/if}

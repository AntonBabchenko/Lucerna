<script lang="ts">
  // Post-update "What's new" dialog: the shared Modal shell wrapping the
  // existing ChangelogPanel, scoped to the versions accumulated since the
  // user's last-seen version. Driven entirely by `whatsNewState` — renders
  // nothing until the prompt toast populates `entries`; closing clears them.
  // Mounted once in +page.svelte.
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import ChangelogPanel from './ChangelogPanel.svelte';
  import { whatsNewState } from './whats-new.svelte';

  function close(): void {
    whatsNewState.entries = null;
  }
</script>

{#if whatsNewState.entries}
  {@const entries = whatsNewState.entries}
  <Modal
    ariaLabelledby="whats-new-title"
    onClose={close}
    panelClass="w-full max-w-2xl max-h-[85vh] flex flex-col"
  >
    <div class="p-4 pb-0 shrink-0 flex items-start justify-between">
      <!-- entries[0] is the version the user just updated to: the slice is
           newest-first and starts at `current`. -->
      <h2 id="whats-new-title" class="text-base font-semibold text-primary flex-1">
        {$t('page.whatsNew.title', { version: entries[0].version })}
      </h2>
      <CloseButton onClick={close} ariaLabel={$t('common.close')} />
    </div>
    <div class="flex-1 overflow-y-auto min-h-0 p-4 pt-3" data-testid="whats-new-body">
      <ChangelogPanel {entries} />
    </div>
  </Modal>
{/if}

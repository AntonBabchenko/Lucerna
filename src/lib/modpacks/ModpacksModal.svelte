<script lang="ts">
  import type { Snippet } from 'svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import HelpPopover from '$lib/ui/HelpPopover.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { explanationState } from '$lib/onboarding/explanation-level.svelte';
  import { explainKey } from '$lib/onboarding/explanation-keys';
  import { t } from '$lib/i18n';

  // Full-viewport modal shell for the modpacks browser. Purely presentational:
  // it owns the scrim, the large centered panel, and the header (title + ×),
  // and projects its body via the `children` snippet. The modpacks browser is
  // NOT instance-scoped (installing a pack creates a new instance), so framing
  // it as a modal over a scrim signals "separate context, not the current
  // instance" — the spatial fix for the modpack/mod confusion.
  //
  // Closing is unconditional and user-driven (× / scrim / Esc) — an import
  // finishing does NOT close the modal, so a mid-browse session isn't
  // interrupted. The import pipeline lives at the PAGE level (via the op-queue
  // store + OperationsView widget), so closing mid-import is also safe: the
  // corner widget keeps running and the page lands the user on the new instance
  // (which takes visible effect once they close the modal themselves).
  let {
    open = false,
    onClose,
    children,
  }: {
    open?: boolean;
    onClose: () => void;
    children?: Snippet;
  } = $props();
</script>

{#if open}
  <Modal
    ariaLabelledby="modpacks-modal-title"
    {onClose}
    panelClass="w-[92vw] max-w-5xl h-[92vh] flex flex-col"
  >
    <header
      class="p-4 border-b border-border-subtle flex items-center gap-1 shrink-0"
      data-testid="modpacks-modal"
    >
      <h2 id="modpacks-modal-title" class="font-semibold text-primary">
        {$t('modpacks.modal.title')}
      </h2>
      <HelpPopover
        body={$t(explainKey('onboarding.modpackInstance.body', explanationState.level))}
        triggerAriaLabel={$t('onboarding.modpackInstance.triggerAriaLabel')}
        triggerTitle={$t('onboarding.modpackInstance.triggerTitle')}
        closeAriaLabel={$t('onboarding.modpackInstance.closeAriaLabel')}
      />
      <span class="flex-1"></span>
      <CloseButton onClick={onClose} ariaLabel={$t('modpacks.modal.closeAriaLabel')} />
    </header>
    <div class="flex-1 overflow-hidden flex flex-col min-h-0">
      {@render children?.()}
    </div>
  </Modal>
{/if}

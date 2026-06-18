<script lang="ts">
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import PreflightPanel from '$lib/mods/PreflightPanel.svelte';
  import type { PreflightReport } from '$lib/ipc/bindings';

  let {
    report,
    busy = false,
    onUpdateLaunch,
    onLaunchAnyway,
    onCancel,
  }: {
    report: PreflightReport;
    busy?: boolean;
    onUpdateLaunch: () => void;
    onLaunchAnyway: () => void;
    onCancel: () => void;
  } = $props();
</script>

<Modal
  ariaLabelledby="preflight-gate-title"
  onClose={onCancel}
  panelClass="max-w-lg w-full p-4"
  closeOnBackdrop={!busy}
  closeOnEscape={!busy}
  dataTestid="preflight-gate-dialog"
>
  <h3 id="preflight-gate-title" class="font-semibold text-lg text-primary mb-3">
    {$t('mods.preflight.gateTitle')}
  </h3>

  <!-- Reuse PreflightPanel for the violation list. The panel's onUpdate and
       onInstallMissing are both no-ops here — at the launch gate, remediation
       is handled by the "Update & launch" button at the dialog level, never
       per-row, so the gate never offers a missing-dependency install. -->
  <div class="mb-4">
    <PreflightPanel {report} onUpdate={() => {}} onInstallMissing={() => {}} />
  </div>

  <div class="flex flex-col gap-2 sm:flex-row sm:justify-end">
    <button
      type="button"
      class="btn-secondary btn-sm w-full sm:w-auto"
      disabled={busy}
      onclick={onCancel}
    >
      {$t('mods.preflight.gateCancel')}
    </button>
    <button
      type="button"
      class="btn-secondary btn-sm w-full sm:w-auto"
      disabled={busy}
      onclick={onLaunchAnyway}
    >
      {$t('mods.preflight.gateLaunchAnyway')}
    </button>
    <BusyButton class="btn-primary btn-sm w-full sm:w-auto" {busy} onclick={onUpdateLaunch}>
      {$t('mods.preflight.gateUpdateLaunch')}
    </BusyButton>
  </div>
</Modal>

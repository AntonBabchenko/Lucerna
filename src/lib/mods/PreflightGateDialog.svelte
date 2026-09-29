<script lang="ts">
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import DialogTitle from '$lib/ui/DialogTitle.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import PreflightPanel from '$lib/mods/PreflightPanel.svelte';
  import type { PreflightReport } from '$lib/ipc/bindings';
  import { hasBlocking } from '$lib/mods/preflight.svelte';
  import { isFixable } from '$lib/mods/violation-view';

  // The Play gate (spec D5, §6.4): what stops the game, and three ways on. Its
  // primary repairs, re-runs the pre-flight and launches only when that is
  // clean; the loop lives with the page (`repairForLaunch`), this only shows it.
  let {
    report,
    instanceId = null,
    busy = false,
    fixed = null,
    onFixAndLaunch,
    onLaunchAnyway,
    onCancel,
  }: {
    report: PreflightReport;
    // Dependency names come from the module name store for this profile — names
    // resolved earlier, never a network call between the user and Play.
    instanceId?: string | null;
    busy?: boolean;
    // «Fixed N of M» after a repair that left rows behind, with why the steps
    // that failed failed (each once); null before one.
    fixed?: { fixed: number; total: number; reasons: readonly string[] } | null;
    onFixAndLaunch: () => void;
    onLaunchAnyway: () => void;
    onCancel: () => void;
  } = $props();

  // Only when the repair has something to act on: a platform mismatch has no
  // automatic fix (spec §6.4), so a gate listing only those offers none.
  const canFix = $derived(hasBlocking(report) && report.violations.some(isFixable));

  // DESIGN.md §8: the pressed «Fix and launch» turns disabled while the repair
  // runs, which drops its focus to <body> — and Tab then walks the page behind
  // the dialog. On the rising edge of `busy`, when focus has left the dialog or
  // sits on a now-disabled control, park it on the body (tabindex=-1, so
  // trapFocus never picks it as the first stop). It stays there if the button
  // is gone afterwards (nothing fixable left).
  let bodyEl = $state<HTMLDivElement | undefined>();
  // Plain variable: the effect's memory of the previous run, not state anything renders.
  let wasBusy = false;
  $effect(() => {
    const now = busy;
    if (now && !wasBusy && bodyEl) {
      const active = document.activeElement;
      const inside = bodyEl.parentElement?.contains(active) ?? false;
      const stranded = active instanceof HTMLButtonElement && active.disabled;
      if (!inside || stranded) bodyEl.focus();
    }
    wasBusy = now;
  });
</script>

<Modal
  ariaLabelledby="preflight-gate-title"
  onClose={onCancel}
  panelClass="max-w-lg w-full p-4"
  closeOnBackdrop={!busy}
  closeOnEscape={!busy}
  dataTestid="preflight-gate-dialog"
>
  <!-- ONE heading: the panel's own title, «What stops the game» — the panel
       below hides its header rather than repeat it. -->
  <DialogTitle id="preflight-gate-title" size="lg" class="mb-3">
    {$t('mods.preflight.panelTitle')}
  </DialogTitle>

  <div
    bind:this={bodyEl}
    tabindex="-1"
    class="flex flex-col gap-3 mb-4 outline-none"
    data-testid="preflight-gate-body"
  >
    <!-- Announced: focus sits on this body while the repair runs. «Fixed 0 of N»
         alone cannot tell a held profile from unrelated failures, so the reasons
         follow in the same announcement. -->
    <StatusMessage
      message={fixed
        ? $t('mods.preflight.gateFixed', { fixed: fixed.fixed, total: fixed.total })
        : null}
      details={fixed?.reasons ?? []}
      tone="info"
      dataTestid="preflight-gate-fixed"
    />
    <!-- The gate only shows what is wrong; it repairs through its primary button. -->
    <PreflightPanel {report} {instanceId} showHeader={false} showRowActions={false} />
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
    {#if canFix}
      <BusyButton class="btn-primary btn-sm w-full sm:w-auto" {busy} onclick={onFixAndLaunch}>
        {$t('mods.preflight.gateFixLaunch')}
      </BusyButton>
    {/if}
  </div>
</Modal>

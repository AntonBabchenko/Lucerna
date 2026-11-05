<script lang="ts">
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import DialogTitle from '$lib/ui/DialogTitle.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import PreflightPanel from '$lib/mods/PreflightPanel.svelte';
  import type { ReasonLine } from '$lib/format/reason-lines';
  import type { PreflightReport } from '$lib/ipc/bindings';
  import { resolveDepNames } from '$lib/mods/dep-names.svelte';
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
    onFinishInstall,
  }: {
    report: PreflightReport;
    // Whose name store names the dependencies. A provider is named by the report itself; a
    // missing one by the platform, asked for as the gate opens (below).
    instanceId?: string | null;
    busy?: boolean;
    // «Fixed N of M» after a repair that left rows behind, with why the steps
    // that failed failed (each once, under the mods it stopped); null before one.
    fixed?: { fixed: number; total: number; reasons: readonly ReasonLine[] } | null;
    onFixAndLaunch: () => void;
    onLaunchAnyway: () => void;
    onCancel: () => void;
    // «Finish install…»: closes the gate without launching and opens the dialog.
    onFinishInstall: () => void;
  } = $props();

  // Missing dependencies are named through their dependent's platform metadata (spec §5.3) — the
  // same cache-first resolver the Installed tab uses, asked here as the gate opens, since on a
  // cold start nothing has asked yet (plan §5b V1). Never waited on: the dialog and its buttons
  // are there at once, and each row shows the loader id only until its name arrives (the store is
  // reactive). A report the repair leaves behind asks only for what is not known yet; the call
  // never rejects.
  $effect(() => {
    if (instanceId) void resolveDepNames(instanceId, report);
  });

  // Only when the repair has something to act on: a platform mismatch has no
  // automatic fix (spec §6.4), so a gate listing only those offers none.
  // Pack files still to download by hand: the gate names them and offers Finish install.
  const pendingFiles = $derived(report.manual_pending ?? []);
  const canFix = $derived(
    hasBlocking(report) && report.violations.some((v) => isFixable(v, pendingFiles.length > 0)),
  );

  // DESIGN.md §8: the pressed «Fix and launch» turns disabled while the repair
  // runs, which drops its focus to <body>: the user's place in the dialog is
  // lost. On the rising edge of `busy`, when focus has left the dialog or
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
    {#if pendingFiles.length > 0}
      <section class="flex flex-col gap-1" data-testid="preflight-gate-manual">
        <h3 class="text-sm font-medium text-primary">
          {$t('modpacks.manual.gate.heading', { count: pendingFiles.length })}
        </h3>
        <p class="text-xs text-secondary">{pendingFiles.map((p) => p.mod_name).join(', ')}</p>
      </section>
    {/if}
    {#if report.violations.length > 0}
      <PreflightPanel {report} {instanceId} showHeader={false} showRowActions={false} />
    {/if}
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
    {#if pendingFiles.length > 0}
      <button
        type="button"
        class="{canFix ? 'btn-secondary' : 'btn-primary'} btn-sm w-full sm:w-auto"
        disabled={busy}
        onclick={onFinishInstall}
      >
        {$t('modpacks.manual.finishInstall')}
      </button>
    {/if}
    {#if canFix}
      <BusyButton class="btn-primary btn-sm w-full sm:w-auto" {busy} onclick={onFixAndLaunch}>
        {$t('mods.preflight.gateFixLaunch')}
      </BusyButton>
    {/if}
  </div>
</Modal>

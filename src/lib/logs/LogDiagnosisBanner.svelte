<script lang="ts">
  import { t } from '$lib/i18n';
  import { commands, type LoaderKind, type RepairChoice, type RepairPlan } from '$lib/ipc/bindings';
  import { DIAGNOSIS_COPY } from '$lib/logs/diagnosis-copy';
  import {
    diagnosisStatus,
    latestDiagnosis,
    refreshDiagnosis,
  } from '$lib/logs/log-diagnosis.svelte';
  import { enqueueRepair } from '$lib/logs/repair-ops.svelte';
  import { deferOrRunRepair } from '$lib/logs/deferred-repairs.svelte';
  import RepairConfirmCard from '$lib/logs/RepairConfirmCard.svelte';
  import MissingModsRepairCard from '$lib/logs/MissingModsRepairCard.svelte';
  import BlockingModsRepairCard from '$lib/logs/BlockingModsRepairCard.svelte';
  import FixModRepairCard from '$lib/logs/FixModRepairCard.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { diagnosisDismiss } from '$lib/ui/diagnosis-dismiss.svelte';
  import { logDiagnosisSignature } from '$lib/logs/log-diagnosis-view';

  let {
    instanceId,
    instanceName,
    mcVersion,
    loader,
    gameRunning = false,
  }: {
    instanceId: string | null;
    instanceName: string | null;
    mcVersion: string | null;
    loader: LoaderKind | null;
    gameRunning?: boolean;
  } = $props();

  const status = $derived(diagnosisStatus());
  const diag = $derived(latestDiagnosis()?.diagnosis ?? null);
  const copy = $derived(diag ? DIAGNOSIS_COPY[diag.pattern_id] : undefined);

  // Dismissal ("I know about this problem"), keyed by the log signature so a
  // different diagnosis resurfaces on its own; the restore badge in LogsPopover
  // brings the same one back.
  const signature = $derived(logDiagnosisSignature(latestDiagnosis()));
  const dismissed = $derived(
    instanceId !== null &&
      signature !== null &&
      diagnosisDismiss.isDismissed(`log:${instanceId}`, signature),
  );

  let repairPlan = $state<RepairPlan | null>(null);
  let repairLoading = $state(false);
  let repairUnavailable = $state(false);
  let repairApplying = $state(false);

  async function startRepair() {
    const path = latestDiagnosis()?.path;
    if (!instanceId || !path) return;
    repairLoading = true;
    repairUnavailable = false;
    repairPlan = null;
    const res = await commands.buildRepairPlan(instanceId, path);
    repairLoading = false;
    if (res.status === 'ok' && res.data) {
      repairPlan = res.data;
    } else {
      repairUnavailable = true;
    }
  }

  async function applyRepair(choice: RepairChoice) {
    if (!instanceId) return;
    repairApplying = true;
    try {
      if (gameRunning) {
        await deferOrRunRepair(true, {
          instanceId,
          sha1: null,
          label: instanceName ?? instanceId,
          choice,
        });
      } else {
        await enqueueRepair(instanceId, instanceName ?? instanceId, choice);
      }
    } finally {
      repairApplying = false;
      repairPlan = null;
      await refreshDiagnosis(instanceId);
    }
  }
</script>

{#if (status === 'actionable' || status === 'advisory') && diag && !dismissed}
  <!-- role="alert" so screen-reader users hear the diagnosis when it appears
       (it renders conditionally after a run, not on mount) — matches the server
       banner. -->
  <div
    class="mb-3 rounded-xl border border-warning-text bg-warning-bg p-3"
    data-testid="log-diagnosis-banner"
    role="alert"
  >
    <div class="flex items-start gap-2">
      <Icon name="warning" class="mt-0.5 text-warning-text" />
      <div class="flex-1">
        <p class="font-semibold text-warning-text">{copy ? $t(copy.title) : diag.title}</p>
        <p class="mt-1 text-sm">{copy ? $t(copy.explanation) : diag.explanation}</p>
        <p class="mt-1 text-sm">
          <span class="font-semibold">{$t('logs.diagnosis.whatToTry')}</span>
          {copy ? $t(copy.recommendation) : diag.recommendation}
        </p>

        {#if status === 'actionable'}
          {#if repairPlan && repairPlan.kind === 'install_missing_mods' && mcVersion && loader}
            <MissingModsRepairCard
              plan={repairPlan}
              instanceId={instanceId ?? ''}
              {mcVersion}
              {loader}
              onClose={() => (repairPlan = null)}
            />
          {:else if repairPlan && repairPlan.kind === 'disable_blocking_mods'}
            <BlockingModsRepairCard
              plan={repairPlan}
              instanceId={instanceId ?? ''}
              {mcVersion}
              {loader}
              {gameRunning}
              onClose={() => (repairPlan = null)}
            />
          {:else if repairPlan && repairPlan.kind === 'install_fix_mod'}
            <FixModRepairCard
              plan={repairPlan}
              busy={repairApplying}
              onConfirm={applyRepair}
              onCancel={() => (repairPlan = null)}
            />
          {:else if repairPlan}
            <RepairConfirmCard
              plan={repairPlan}
              busy={repairApplying}
              onConfirm={applyRepair}
              onCancel={() => (repairPlan = null)}
            />
          {:else}
            <BusyButton
              type="button"
              class="btn-primary btn-sm mt-2"
              data-testid="diagnosis-fix"
              busy={repairLoading}
              onclick={startRepair}
            >
              {repairLoading ? $t('logs.repair.checking') : $t('logs.repair.fixThis')}
            </BusyButton>
            {#if repairUnavailable}
              <p class="mt-2 text-xs text-secondary">{$t('logs.repair.unavailable')}</p>
            {/if}
          {/if}
        {/if}
      </div>
      <button
        type="button"
        class="btn-icon -my-1 -mr-1 shrink-0 !text-warning-text hover:!bg-warning-text/10"
        aria-label={$t('common.dismissWarning')}
        use:tooltip={$t('common.dismissWarningTooltip')}
        onclick={() =>
          instanceId && signature && diagnosisDismiss.dismiss(`log:${instanceId}`, signature)}
        data-testid="log-diagnosis-dismiss"><Icon name="close" size={16} /></button
      >
    </div>
  </div>
{/if}

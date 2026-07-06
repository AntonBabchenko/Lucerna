<script lang="ts">
  // Live progress for an in-flight data-root relocation, driven by the
  // `dataMigrationProgress` event (phase: copying → verifying → deleting).
  // A successful migration ends in `app.restart()`, so this dialog is
  // normally torn down by the restart itself — it exists mainly to keep the
  // user informed (and not double-click anything) during the copy, which can
  // take a while for a large data root. No close affordance: the operation
  // cannot be safely cancelled mid-flight.
  import Modal from '$lib/ui/Modal.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { t } from '$lib/i18n';
  import type { DataMigrationProgress } from '$lib/ipc/bindings';

  let { progress }: { progress: DataMigrationProgress | null } = $props();

  function pct(p: DataMigrationProgress): number {
    if (!p.total_bytes || p.total_bytes <= 0) return 0;
    return Math.min(100, Math.round(((p.copied_bytes ?? 0) / p.total_bytes) * 100));
  }

  function phaseLabel(p: DataMigrationProgress | null): string {
    if (!p) return $t('settings.storage.dataLocation.progress.preparing');
    if (p.phase === 'verifying') return $t('settings.storage.dataLocation.progress.verifying');
    if (p.phase === 'deleting') return $t('settings.storage.dataLocation.progress.deleting');
    return $t('settings.storage.dataLocation.progress.copying');
  }
</script>

<Modal
  ariaLabelledby="data-location-progress-title"
  onClose={() => {}}
  closeOnBackdrop={false}
  closeOnEscape={false}
  panelClass="w-[440px] p-5 flex flex-col gap-3"
>
  <h3 id="data-location-progress-title" class="font-semibold text-primary text-base">
    {$t('settings.storage.dataLocation.progress.title')}
  </h3>
  <div class="flex items-center gap-2 text-sm text-secondary" role="status" aria-live="polite">
    <Spinner size="sm" />
    <span>{phaseLabel(progress)}</span>
  </div>
  {#if progress && progress.phase === 'copying' && progress.total_bytes && progress.total_bytes > 0}
    <div class="h-2 bg-subtle rounded overflow-hidden">
      <div
        class="h-full bg-accent"
        style="width: {pct(progress)}%"
        data-testid="data-location-progress-bar"
      ></div>
    </div>
  {/if}
</Modal>

<script lang="ts">
  import { t } from '$lib/i18n';

  // Determinate progress for an in-flight modpack update. `progress === null`
  // is the pre-first-event "preparing" state (also a removal-only update that
  // emits no per-file events) — label only, no bar. Bar markup mirrors
  // OperationsBar.svelte for visual consistency.
  let {
    progress,
  }: {
    progress: { current: number; total: number; fileName: string } | null;
  } = $props();

  function pct(done: number, total: number): number {
    if (total <= 0) return 0;
    return Math.min(100, Math.round((done / total) * 100));
  }
</script>

<div class="flex flex-col gap-2" data-testid="imported-detail-updating">
  <div class="text-sm text-accent truncate">
    {#if progress}
      {$t('modpacks.imported.detail.updateProgressDownloading', {
        current: progress.current,
        total: progress.total,
        fileName: progress.fileName,
      })}
    {:else}
      {$t('modpacks.imported.detail.updating')}
    {/if}
  </div>
  {#if progress && progress.total > 0}
    <div class="h-2 bg-subtle rounded overflow-hidden">
      <div
        class="h-full bg-accent"
        style="width: {pct(progress.current, progress.total)}%"
        data-testid="imported-detail-update-bar"
      ></div>
    </div>
  {/if}
</div>

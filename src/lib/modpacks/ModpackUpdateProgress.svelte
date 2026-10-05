<script lang="ts">
  import { t } from '$lib/i18n';
  import type { UpdateFileProgress } from './modpack-update-flow.svelte';

  // Progress for an in-flight modpack update. `progress === null` is the
  // pre-first-event "preparing" state (also a removal-only update that emits no
  // per-file events) — label only, no bar; so is `applying_changes`, the
  // all-or-nothing step that moves the new files in and has no count of its
  // own. Bar markup mirrors OperationsBar.svelte for visual consistency. While
  // worlds are backed up `fileName` is the world's folder name.
  let { progress }: { progress: UpdateFileProgress | null } = $props();

  // The bar's share. A download counts the file being fetched: many short steps.
  // A world backup counts the worlds already zipped: one long step per world, so
  // counting the one in progress showed a profile's only world as a full bar
  // for as long as it was being zipped.
  const bar = $derived.by((): { done: number; total: number } | null => {
    if (progress === null || progress.phase === 'applying_changes' || progress.total <= 0) {
      return null;
    }
    const done = progress.phase === 'backing_up_world' ? progress.current - 1 : progress.current;
    return { done, total: progress.total };
  });

  function pct(done: number, total: number): number {
    if (total <= 0) return 0;
    return Math.max(0, Math.min(100, Math.round((done / total) * 100)));
  }
</script>

<div class="flex flex-col gap-2" data-testid="imported-detail-updating">
  <div class="text-sm text-accent truncate">
    {#if progress && progress.phase === 'applying_changes'}
      {$t('modpacks.imported.detail.updateProgressApplying')}
    {:else if progress && progress.phase === 'backing_up_world'}
      {$t('modpacks.imported.detail.updateProgressBackingUp', {
        current: progress.current,
        total: progress.total,
        worldName: progress.fileName,
      })}
    {:else if progress}
      {$t('modpacks.imported.detail.updateProgressDownloading', {
        current: progress.current,
        total: progress.total,
        fileName: progress.fileName,
      })}
    {:else}
      {$t('modpacks.imported.detail.updating')}
    {/if}
  </div>
  {#if bar}
    <div class="h-2 bg-subtle rounded overflow-hidden">
      <div
        class="h-full bg-accent"
        style="width: {pct(bar.done, bar.total)}%"
        data-testid="imported-detail-update-bar"
      ></div>
    </div>
  {/if}
</div>

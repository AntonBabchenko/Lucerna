<script lang="ts">
  import { t } from 'svelte-i18n';
  import { integrityQueueLength, integrityRunning } from '$lib/instances/integrity-ops.svelte';

  // Render-only page-level progress for the running integrity op, mirroring
  // ImportProgressView. The op is owned by the integrity-ops store; this view
  // just reads `integrityRunning()` / `integrityQueueLength()` (read at call
  // time → reactive). Renders nothing when no op is running.

  const running = $derived(integrityRunning());
  const queued = $derived(integrityQueueLength());

  function percent(done: number, total: number): number {
    if (total <= 0) return 0;
    return Math.min(100, Math.round((done / total) * 100));
  }
</script>

{#if running}
  <div
    class="fixed top-4 right-4 z-40 w-72 bg-surface rounded-lg shadow-xl border p-4"
    role="status"
    aria-label={$t('instance.integrity.heading')}
    data-testid="integrity-progress-view"
  >
    <h3 class="font-semibold text-sm text-primary mb-1">
      {$t('instance.integrity.heading')}
    </h3>
    <div class="text-sm text-secondary truncate">
      {running.kind === 'verify'
        ? $t('instance.integrity.opVerifying', { values: { name: running.name } })
        : $t('instance.integrity.opRepairing', { values: { name: running.name } })}
      {running.filesDone}/{running.filesTotal}
    </div>
    <div class="h-2 bg-subtle rounded mt-2 overflow-hidden">
      <div
        class="h-full bg-accent"
        style="width: {percent(running.filesDone, running.filesTotal)}%"
      ></div>
    </div>
    {#if queued > 0}
      <div class="text-xs text-muted mt-1">
        (+{$t('instance.integrity.opQueued', { values: { count: queued } })})
      </div>
    {/if}
  </div>
{/if}

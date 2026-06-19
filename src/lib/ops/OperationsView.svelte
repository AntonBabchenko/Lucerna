<script lang="ts">
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import type { ModpackProgress } from '$lib/ipc/bindings';
  import { cancelQueued, moveQueued, opQueue, opRunning, type QueuedOp } from './op-queue.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import Spinner from '$lib/ui/Spinner.svelte';
  import Icon from '$lib/ui/icons/Icon.svelte';

  // Unified page-level "Operations" widget. Replaces IntegrityProgressView +
  // ImportProgressView: one corner card showing the running op's progress plus
  // an expandable list of queued ops with reorder/cancel controls. Renders
  // nothing when nothing is running and nothing is queued.

  const running = $derived(opRunning());
  const queue = $derived(opQueue());
  let expanded = $state(true);

  function pct(done: number, total: number): number {
    if (total <= 0) return 0;
    return Math.min(100, Math.round((done / total) * 100));
  }

  function runningLabel(op: QueuedOp): string {
    if (op.kind === 'import') return $t('modpacks.import.progress.heading');
    if (op.kind === 'launcher-import') return $t('instances.import.progress', { name: op.name });
    return op.kind === 'verify'
      ? $t('instance.integrity.opVerifying', { name: op.name })
      : $t('instance.integrity.opRepairing', { name: op.name });
  }

  function importPhaseLabel(phase: ModpackProgress): string {
    const tr = get(t);
    switch (phase.phase) {
      case 'inspecting':
        return tr('modpacks.import.progress.phaseInspecting');
      case 'creating_instance':
        return tr('modpacks.import.progress.phaseCreatingInstance', { name: phase.name });
      case 'installing_file':
        return tr('modpacks.import.progress.phaseInstallingFile', {
          current: phase.current,
          total: phase.total,
          fileName: phase.file_name,
        });
      case 'extracting_overrides':
        return tr('modpacks.import.progress.phaseExtractingOverrides', {
          current: phase.current,
          total: phase.total,
        });
      case 'enriching':
        return tr('modpacks.import.progress.phaseEnriching');
      case 'done':
        return tr('modpacks.import.progress.phaseDone');
    }
  }

  function queueItemLabel(op: QueuedOp): string {
    if (op.kind === 'import') return $t('ops.queueItemImport', { name: op.name });
    if (op.kind === 'launcher-import') return $t('ops.queueItemLauncherImport', { name: op.name });
    return op.kind === 'verify'
      ? $t('ops.queueItemVerify', { name: op.name })
      : $t('ops.queueItemRepair', { name: op.name });
  }
</script>

{#if running || queue.length > 0}
  <div
    class="fixed top-4 right-4 z-40 w-72 bg-surface rounded-lg shadow-xl border p-4"
    role="status"
    aria-label={$t('ops.heading')}
    data-testid="operations-view"
  >
    <h3 class="font-semibold text-sm text-primary mb-1">{$t('ops.heading')}</h3>

    {#if running}
      <div class="flex items-center gap-2">
        <Spinner size="sm" />
        <div
          class="text-sm text-secondary truncate flex-1"
          use:tooltip={{ text: runningLabel(running.op), whenOverflowing: true }}
        >
          {runningLabel(running.op)}
        </div>
      </div>
      {#if running.progress.kind !== 'import' && running.progress.kind !== 'launcher-import'}
        <div class="text-xs text-muted">
          {running.progress.filesDone}/{running.progress.filesTotal}
        </div>
        <div class="h-2 bg-subtle rounded mt-2 overflow-hidden">
          <div
            class="h-full bg-accent"
            style="width: {pct(running.progress.filesDone, running.progress.filesTotal)}%"
          ></div>
        </div>
      {:else if running.progress.kind === 'import'}
        {#if running.progress.phase}
          <div
            class="text-xs text-muted truncate"
            use:tooltip={{ text: importPhaseLabel(running.progress.phase), whenOverflowing: true }}
          >
            {importPhaseLabel(running.progress.phase)}
          </div>
        {/if}
        {#if running.progress.bytes && running.progress.bytes.total && running.progress.bytes.total > 0 && running.progress.bytes.current != null}
          <div class="h-2 bg-subtle rounded mt-2 overflow-hidden">
            <div
              class="h-full bg-accent"
              style="width: {pct(running.progress.bytes.current, running.progress.bytes.total)}%"
            ></div>
          </div>
        {/if}
      {:else if running.progress.kind === 'launcher-import'}
        {#if running.progress.phase}
          <div class="text-xs text-muted truncate">
            {$t(`instances.import.phase.${running.progress.phase.phase}`)}
          </div>
        {/if}
      {/if}
    {/if}

    {#if queue.length > 0}
      <button
        type="button"
        class="inline-flex items-center gap-1 text-xs text-muted mt-2 hover:text-secondary"
        aria-expanded={expanded}
        onclick={() => (expanded = !expanded)}
      >
        <Icon name={expanded ? 'chevronDown' : 'caret'} size={14} />
        {$t('ops.inQueue', { count: queue.length })}
      </button>
      {#if expanded}
        <ul class="mt-1 space-y-1">
          {#each queue as op, i (op.id)}
            <li class="flex items-center gap-1 text-xs text-secondary">
              <span
                class="truncate flex-1"
                use:tooltip={{ text: queueItemLabel(op), whenOverflowing: true }}
                >{queueItemLabel(op)}</span
              >
              <button
                type="button"
                class="btn-icon btn-icon-sm"
                disabled={i === 0}
                aria-label={$t('ops.moveUp')}
                use:tooltip={$t('ops.moveUp')}
                onclick={() => moveQueued(op.id, 'up')}
              >
                <Icon name="chevronUp" size={14} />
              </button>
              <button
                type="button"
                class="btn-icon btn-icon-sm"
                disabled={i === queue.length - 1}
                aria-label={$t('ops.moveDown')}
                use:tooltip={$t('ops.moveDown')}
                onclick={() => moveQueued(op.id, 'down')}
              >
                <Icon name="chevronDown" size={14} />
              </button>
              <button
                type="button"
                class="btn-icon btn-icon-sm btn-icon-danger"
                aria-label={$t('ops.cancel')}
                use:tooltip={$t('ops.cancel')}
                onclick={() => cancelQueued(op.id)}
              >
                <Icon name="close" size={14} />
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  </div>
{/if}

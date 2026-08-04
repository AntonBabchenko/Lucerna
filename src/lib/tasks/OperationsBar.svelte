<script lang="ts">
  // The collapsed bottom strip — the single visible anchor for every
  // long-running task the registry tracks (game install, mod install/
  // update, pack import/update, launcher import, clone, verify/repair,
  // server upload, app update, data migration). Replaces both today's
  // corner card (`$lib/ops/OperationsView.svelte`) and the event-driven
  // `$lib/install/PhaseStatusRow.svelte`; wiring +page.svelte to mount
  // this instead, and removing those two, is a later task's job.
  //
  // Scope is the session's active (`queued`/`running`) OR finished tasks —
  // NOT active-only. A task's finished report (the expanded panel this
  // strip discloses, and eventually the report modal a row's Details opens)
  // must stay reachable after the task ends, which an active-only filter
  // would make impossible the moment the last active task finishes. When
  // nothing is active, the strip shows a finished-tasks summary in place of
  // the spinner/progress readout; the panel's Finished-section "Clear"
  // control (not hiding the strip) is what eventually lets it go quiet
  // again.
  //
  // `Task.phase` (see `./types.ts`) is a raw, kind-specific backend enum
  // string with no single shared vocabulary across the twelve task kinds —
  // `./phase-label.ts` is that per-kind dispatch, reusing the i18n mappings
  // that already existed scattered across `PhaseStatusRow.svelte` and
  // `OperationsView.svelte` (the two components this strip replaces) plus
  // `DataLocationProgressDialog.svelte` (which is NOT being replaced). The
  // kind label (`KIND_LABEL_KEY`) still anchors the "operations-bar-kind"
  // reading; the phase text is a second, narrower line under it for kinds
  // that have one.
  import { t } from '$lib/i18n';
  import { formatSize } from '$lib/format/size';
  import { formatEtaClock } from '$lib/servers/upload-progress-format';
  import { isActiveTask, taskList } from './registry.svelte';
  import { canShowRate } from './rate';
  import { phaseLabel } from './phase-label';
  import { KIND_LABEL_KEY } from './types';
  import type { Task } from './types';
  import OperationsPanel from './OperationsPanel.svelte';
  import TaskReportModal from './TaskReportModal.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import Icon from '$lib/ui/icons/Icon.svelte';
  import { tooltip } from '$lib/ui/tooltip';

  let {
    onDetails,
  }: {
    onDetails?: (task: Task) => void;
  } = $props();

  // Disclosure state for the panel this strip mounts below.
  let expanded = $state(false);

  // The report modal a row's Details click opens. `onDetails` stays a plain
  // callback prop (OperationsPanel's existing contract — see its module doc
  // comment) so a future host can still intercept it; absent an override,
  // this IS the default behaviour, since otherwise Details would be a dead
  // end.
  let reportTask = $state<Task | null>(null);

  function handleDetails(task: Task) {
    if (onDetails) {
      onDetails(task);
    } else {
      reportTask = task;
    }
  }

  const allTasks = $derived(taskList());
  const activeTasks = $derived(allTasks.filter(isActiveTask));
  const finishedTasks = $derived(allTasks.filter((task) => !isActiveTask(task)));
  const hasAnyTask = $derived(activeTasks.length > 0 || finishedTasks.length > 0);

  // Earliest-started of the active tasks — see the module doc comment for
  // why this filter is what keeps the selection stable while it runs.
  const displayTask = $derived(
    activeTasks.length === 0
      ? null
      : activeTasks.reduce((earliest, task) =>
          task.startedAt < earliest.startedAt ? task : earliest,
        ),
  );

  function percent(current: number, total: number): number {
    if (total <= 0) return 0;
    return Math.min(100, Math.round((current / total) * 100));
  }
</script>

{#if hasAnyTask}
  <div
    class="relative border-t bg-base px-4 py-1 flex items-center gap-3 text-xs"
    role="status"
    aria-live="polite"
    aria-label={$t('ops.heading')}
    data-testid="operations-bar"
  >
    {#if displayTask}
      {@const task = displayTask}
      {@const count = activeTasks.length}
      {@const kindLabel = $t(KIND_LABEL_KEY[task.kind])}
      {@const phase = phaseLabel($t, task)}
      {@const byteRate = canShowRate(task.progress)}
      {@const fileCount =
        !byteRate &&
        task.progress !== null &&
        task.progress.unit === 'files' &&
        task.progress.total > 0}
      <Spinner size="sm" />
      {#if count > 1}
        <span class="font-medium text-primary" data-testid="operations-bar-count">
          {$t('tasks.strip.count', { count })}
        </span>
      {/if}
      <span class="font-medium text-primary truncate" data-testid="operations-bar-kind">
        {kindLabel}
      </span>
      {#if phase}
        <span class="text-secondary truncate" data-testid="operations-bar-phase">
          {phase}
        </span>
      {/if}

      {#if byteRate}
        <span class="text-secondary font-mono" data-testid="operations-bar-rate">
          {$t('format.size.perSecond', { size: formatSize($t, task.rate?.bytesPerSec ?? 0) })}
        </span>
        <span class="text-secondary font-mono" data-testid="operations-bar-eta">
          {$t('tasks.strip.eta', { clock: formatEtaClock(task.rate?.etaSeconds ?? null) })}
        </span>
        <div
          class="flex-1 h-1 bg-subtle rounded overflow-hidden"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent(task.progress?.current ?? 0, task.progress?.total ?? 0)}
          data-testid="operations-bar-progress"
        >
          <div
            class="h-full bg-accent transition-all"
            style="width: {percent(task.progress?.current ?? 0, task.progress?.total ?? 0)}%"
          ></div>
        </div>
      {:else if fileCount}
        <span class="text-secondary font-mono" data-testid="operations-bar-counter">
          {task.progress?.current}/{task.progress?.total}
          ({percent(task.progress?.current ?? 0, task.progress?.total ?? 0)}%)
        </span>
      {:else}
        <div
          class="flex-1 h-1 bg-subtle rounded overflow-hidden"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          data-testid="operations-bar-progress"
        >
          <div class="h-full w-1/3 animate-pulse rounded-full bg-accent"></div>
        </div>
      {/if}
    {:else}
      <span class="font-medium text-primary" data-testid="operations-bar-finished-summary">
        {$t('tasks.strip.finishedSummary', { count: finishedTasks.length })}
      </span>
      <div class="flex-1"></div>
    {/if}

    <button
      type="button"
      class="btn-icon btn-icon-sm"
      aria-expanded={expanded}
      aria-label={$t('tasks.strip.toggle')}
      use:tooltip={$t('tasks.strip.toggle')}
      data-testid="operations-bar-toggle"
      onclick={() => (expanded = !expanded)}
    >
      <Icon name={expanded ? 'chevronDown' : 'caret'} size={14} />
    </button>

    {#if expanded}
      <OperationsPanel onClose={() => (expanded = false)} onDetails={handleDetails} />
    {/if}
  </div>
{/if}

{#if reportTask}
  <TaskReportModal task={reportTask} onClose={() => (reportTask = null)} />
{/if}

<script lang="ts">
  // The collapsed bottom strip — the single visible anchor for every
  // long-running task the registry tracks (game install, mod install/
  // update, pack import/update, launcher import, clone, verify/repair,
  // server upload, app update, data migration). Replaces both the former
  // corner card (`$lib/ops/OperationsView.svelte`) and the event-driven
  // `$lib/install/PhaseStatusRow.svelte` — both retired; `+page.svelte`
  // mounts this in their place (see the comment at that mount for the grid
  // placement + DOM-order/stacking contract it has to keep).
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

    <span class="btn-icon btn-icon-sm" data-testid="operations-bar-indicator" aria-hidden="true">
      <Icon name={expanded ? 'chevronDown' : 'caret'} size={14} />
    </span>

    <!-- The disclosure control is the WHOLE strip, not the chevron. It cannot
         be a wrapping <button>: the strip contains a progressbar and mounts
         OperationsPanel below, and neither may live inside a button. So the
         button is stretched over the strip instead — transparent, so the
         content stays visible through it; paint order captures only the
         clicks, not the pixels.

         MUST stay before OperationsPanel in DOM order. Both are positioned;
         the panel carries `z-[var(--z-popover)]` and therefore paints above
         this overlay, which is what keeps a click inside the open panel from
         landing here and collapsing it. Move this after the panel and every
         click in the panel closes it instead — covered by the DOM-order test
         in tests/tasks/operations-bar.test.ts.

         No `use:tooltip` here: anchored to a full-width strip it would follow
         the pointer across the entire bar. The accessible name lives on the
         button itself. -->
    <button
      type="button"
      class="absolute inset-0"
      aria-expanded={expanded}
      aria-label={$t('tasks.strip.toggle')}
      data-testid="operations-bar-toggle"
      onclick={() => (expanded = !expanded)}
    ></button>

    {#if expanded}
      <OperationsPanel onClose={() => (expanded = false)} onDetails={handleDetails} />
    {/if}
  </div>
{/if}

{#if reportTask}
  <TaskReportModal task={reportTask} onClose={() => (reportTask = null)} />
{/if}

<script lang="ts">
  // The collapsed bottom strip — the single visible anchor for every
  // long-running task the registry tracks (game install, mod install/
  // update, pack import/update, launcher import, clone, verify/repair,
  // server upload, app update, data migration). Replaces both today's
  // corner card (`$lib/ops/OperationsView.svelte`) and the event-driven
  // `$lib/install/PhaseStatusRow.svelte`; wiring +page.svelte to mount
  // this instead, and removing those two, is a later task's job.
  //
  // Scope is deliberately the ACTIVE tasks only (`queued`/`running`) — a
  // finished task is history, not "long-running work in flight": the
  // report modal a later task builds is where it lives on, not here.
  // That restriction is also what makes "the displayed task changes only
  // when it finishes" true for free: the earliest-started task of this
  // filtered set can only stop being the minimum by dropping out of the
  // filter (reaching a terminal state) — never because some other task's
  // progress ticked or a third task started even later.
  //
  // `Task.phase` (see `./types.ts`) is a raw, kind-specific backend enum
  // string (`downloading` / `installing_file` / `verifying` / a bare file
  // path for server-upload, ...) with no shared i18n mapping across the
  // twelve task kinds — building one is separate, larger work (each
  // existing per-surface component that reads a phase today, e.g.
  // `PhaseStatusRow`, hand-rolls its own switch for its one kind's phase
  // space). The localized "what is happening" label shown here is the
  // kind label (`KIND_LABEL_KEY`) — the vocabulary `types.ts` already
  // documents as the one every task surface speaks.
  import { t } from '$lib/i18n';
  import { formatSize } from '$lib/format/size';
  import { formatEtaClock } from '$lib/servers/upload-progress-format';
  import { taskList } from './registry.svelte';
  import { canShowRate } from './rate';
  import { KIND_LABEL_KEY } from './types';
  import Spinner from '$lib/ui/Spinner.svelte';
  import Icon from '$lib/ui/icons/Icon.svelte';
  import { tooltip } from '$lib/ui/tooltip';

  // Disclosure state for the panel a later task builds — this component
  // only owns exposing the button + `aria-expanded`.
  let expanded = $state(false);

  const activeTasks = $derived(
    taskList().filter((task) => task.state === 'queued' || task.state === 'running'),
  );

  // Earliest-started of the active tasks — see the module doc comment for
  // why this filter is what keeps the selection stable while it runs.
  const displayTask = $derived(
    activeTasks.length === 0
      ? null
      : activeTasks.reduce((earliest, task) => (task.startedAt < earliest.startedAt ? task : earliest)),
  );

  function percent(current: number, total: number): number {
    if (total <= 0) return 0;
    return Math.min(100, Math.round((current / total) * 100));
  }
</script>

{#if displayTask}
  {@const task = displayTask}
  {@const count = activeTasks.length}
  {@const kindLabel = $t(KIND_LABEL_KEY[task.kind])}
  {@const byteRate = canShowRate(task.progress)}
  {@const fileCount =
    !byteRate &&
    task.progress !== null &&
    task.progress.unit === 'files' &&
    task.progress.total > 0}
  <div
    class="border-t bg-base px-4 py-1 flex items-center gap-3 text-xs"
    role="status"
    aria-live="polite"
    aria-label={$t('ops.heading')}
    data-testid="operations-bar"
  >
    <Spinner size="sm" />
    {#if count > 1}
      <span class="font-medium text-primary" data-testid="operations-bar-count">
        {$t('tasks.strip.count', { count })}
      </span>
    {/if}
    <span class="font-medium text-primary truncate" data-testid="operations-bar-kind">
      {kindLabel}
    </span>

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
  </div>
{/if}

<script lang="ts">
  // The expanded panel behind `OperationsBar.svelte`'s disclosure toggle —
  // every task the registry tracks this session, not just the one the
  // collapsed strip summarises. Mounted directly by `OperationsBar` as an
  // absolutely-positioned overlay (`bottom-full` on a `relative` strip), so
  // it opens upward without pushing the strip or the page content it sits
  // above — unlike the app shell's normal-flow rows.
  //
  // Reads the registry itself (`taskList()`) rather than taking a `tasks`
  // prop, same module-singleton idiom `OperationsBar` already uses — so
  // `cancelQueued` / `moveQueued` / `clearFinished` calls made from a row
  // here are visible immediately without any prop plumbing back up.
  //
  // Deliberately NOT built on `attachPopoverDismiss` (the close-on-scroll/
  // resize wiring `Select`/`Menu`/`HelpPopover` share, see
  // `$lib/ui/popover-dismiss.ts`): this panel is wanted precisely while the
  // user scrolls a mod list or the main content area, so a page scroll must
  // never dismiss it. It closes only two ways — the strip's toggle firing a
  // second time (that state lives in `OperationsBar`, not here) or Escape
  // (wired locally below) — never on scroll, and scrolling the panel's own
  // internal `overflow-y-auto` body is not exempted from anything because
  // nothing here listens for scroll at all.
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/icons/Icon.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { phaseLabel } from './phase-label';
  import {
    cancelQueued,
    clearFinished,
    isActiveTask,
    moveQueued,
    taskList,
  } from './registry.svelte';
  import { KIND_LABEL_KEY } from './types';
  import type { Task } from './types';

  let {
    onClose,
    onDetails,
  }: {
    onClose: () => void;
    onDetails?: (task: Task) => void;
  } = $props();

  const allTasks = $derived(taskList());
  const running = $derived(allTasks.filter((task) => task.state === 'running'));
  const queued = $derived(allTasks.filter((task) => task.state === 'queued'));
  // Finished = every terminal state, newest first — active work (Running /
  // Queued, rendered above) stays pinned at the top even once this list is
  // long enough to scroll.
  const finished = $derived(
    allTasks
      .filter((task) => !isActiveTask(task))
      .slice()
      .sort((a, b) => (b.finishedAt ?? 0) - (a.finishedAt ?? 0)),
  );

  $effect(() => {
    function onKeydown(e: KeyboardEvent): void {
      if (e.key !== 'Escape') return;
      // Mirrors Select's own Escape handling: stop propagation so an
      // enclosing modal (if this panel is ever opened while one happens to
      // be up) doesn't also treat the same keypress as its own close.
      e.stopPropagation();
      onClose();
    }
    window.addEventListener('keydown', onKeydown);
    return () => window.removeEventListener('keydown', onKeydown);
  });
</script>

{#snippet row(task: Task, index: number, total: number)}
  {@const kindLabel = $t(KIND_LABEL_KEY[task.kind])}
  {@const phase = phaseLabel($t, task)}
  <li
    class="relative flex items-center gap-2 rounded px-2 py-1.5 text-xs {task.details !== null
      ? 'cursor-pointer hover:bg-subtle'
      : ''}"
    data-testid={`operations-panel-row-${task.id}`}
  >
    {#if task.state === 'running'}
      <Spinner size="sm" />
    {/if}
    <div class="min-w-0 flex-1">
      <div class="truncate font-medium text-primary">{kindLabel} · {task.title}</div>
      {#if phase}
        <div class="truncate text-secondary">{phase}</div>
      {/if}
    </div>

    {#if task.lane === 'modal'}
      <span
        use:tooltip={$t('tasks.panel.modalLock')}
        data-testid={`operations-panel-lock-${task.id}`}
      >
        <Icon name="lock" size={14} label={$t('tasks.panel.modalLock')} />
      </span>
    {/if}

    {#if task.caps.reorderable}
      <span class="relative z-10" use:tooltip={$t('ops.moveUp')}>
        <button
          type="button"
          class="btn-icon btn-icon-sm"
          disabled={index === 0}
          aria-label={$t('ops.moveUp')}
          data-testid={`operations-panel-move-up-${task.id}`}
          onclick={() => moveQueued(task.id, 'up')}
        >
          <Icon name="chevronUp" size={14} />
        </button>
      </span>
      <span class="relative z-10" use:tooltip={$t('ops.moveDown')}>
        <button
          type="button"
          class="btn-icon btn-icon-sm"
          disabled={index === total - 1}
          aria-label={$t('ops.moveDown')}
          data-testid={`operations-panel-move-down-${task.id}`}
          onclick={() => moveQueued(task.id, 'down')}
        >
          <Icon name="chevronDown" size={14} />
        </button>
      </span>
    {/if}

    {#if task.caps.cancellable}
      <button
        type="button"
        class="btn-icon btn-icon-sm btn-icon-danger relative z-10"
        aria-label={$t('ops.cancel')}
        use:tooltip={$t('ops.cancel')}
        data-testid={`operations-panel-cancel-${task.id}`}
        onclick={() => cancelQueued(task.id)}
      >
        <Icon name="close" size={14} />
      </button>
    {/if}

    {#if task.details !== null}
      <span
        class="relative z-10 text-muted"
        data-testid={`operations-panel-chevron-${task.id}`}
        aria-hidden="true"
      >
        <Icon name="caret" size={14} />
      </span>
      <!-- The report control is the whole row, as a stretched button rather
           than an onclick on the <li>. The reason is the keyboard: a list item
           is not focusable and fires no click on Enter/Space, so a handler
           there would be mouse-only. Pinned by the "keeps the report control
           keyboard-operable" test.

           Bubbling from the row's own move/cancel buttons is NOT a concern
           here, and no test claims otherwise: `capsFor` grants those caps only
           while a task is `queued`, and `details` only exist once it has
           finished, so no row ever carries both. The `z-10` on those controls
           is cheap insurance if that ever stops being true, not a guarantee. -->
      <button
        type="button"
        class="absolute inset-0 rounded"
        aria-label={$t('tasks.panel.details')}
        data-testid={`operations-panel-details-${task.id}`}
        onclick={() => onDetails?.(task)}
      ></button>
    {/if}
  </li>
{/snippet}

<div
  class="absolute inset-x-0 bottom-full z-[var(--z-popover)] mb-1 max-h-[45vh] overflow-y-auto rounded-t-lg border bg-surface shadow-xl"
  role="region"
  aria-label={$t('tasks.panel.ariaLabel')}
  data-testid="operations-panel"
>
  {#if running.length > 0}
    <div class="border-b p-2">
      <h3
        class="px-1 pb-1 text-xs font-semibold uppercase tracking-wide text-muted"
        data-testid="operations-panel-section-running"
      >
        {$t('tasks.panel.runningHeading')}
      </h3>
      <ul class="flex flex-col gap-1">
        {#each running as task, i (task.id)}
          {@render row(task, i, running.length)}
        {/each}
      </ul>
    </div>
  {/if}

  {#if queued.length > 0}
    <div class="border-b p-2">
      <h3
        class="px-1 pb-1 text-xs font-semibold uppercase tracking-wide text-muted"
        data-testid="operations-panel-section-queued"
      >
        {$t('tasks.panel.queuedHeading')}
      </h3>
      <ul class="flex flex-col gap-1">
        {#each queued as task, i (task.id)}
          {@render row(task, i, queued.length)}
        {/each}
      </ul>
    </div>
  {/if}

  {#if finished.length > 0}
    <div class="p-2">
      <div class="flex items-center justify-between px-1 pb-1">
        <h3
          class="text-xs font-semibold uppercase tracking-wide text-muted"
          data-testid="operations-panel-section-finished"
        >
          {$t('tasks.panel.finishedHeading')}
        </h3>
        <button
          type="button"
          class="btn-tertiary btn-xs"
          data-testid="operations-panel-clear-finished"
          onclick={() => clearFinished()}
        >
          {$t('tasks.panel.clearFinished')}
        </button>
      </div>
      <ul class="flex flex-col gap-1">
        {#each finished as task, i (task.id)}
          {@render row(task, i, finished.length)}
        {/each}
      </ul>
    </div>
  {/if}
</div>

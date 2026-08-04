<script lang="ts">
  // The install report — what the whole tasks/ package exists for. Opened
  // from OperationsPanel's per-row "Details" button (`onDetails: (task:
  // Task) => void`, see OperationsBar.svelte), this reads `task.details`
  // (a TaskDetail[] persisted per-file report) and lets the user filter by
  // outcome, expand a row for its full provenance, and copy the current
  // filtered view as text.
  //
  // The one fact this component must never blur: `fetched` and `placement`
  // on an `installed` outcome are ORTHOGONAL (see DetailOutcome's doc
  // comment in `$lib/ipc/bindings.ts`). `fetch_to_cache` always ensures the
  // sha-keyed file exists in the shared content cache — downloading only on
  // a miss — and `materialize` then hardlinks *from that cache* either way,
  // so a jar downloaded seconds ago is ALSO `linked`. Every
  // fetched×placement combination therefore gets its own explicit copy in
  // `outcomeDetailText` below; collapsing any of them would make "linked"
  // read as "was not downloaded", which is false, or vice-versa.
  import { t } from '$lib/i18n';
  import { formatSize } from '$lib/format/size';
  import Modal from '$lib/ui/Modal.svelte';
  import ToggleChipGroup from '$lib/ui/ToggleChipGroup.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import type { BadgeVariant } from '$lib/ui/cards/card-status';
  import { Icon } from '$lib/ui/icons';
  import type { DetailOutcome, TaskDetail } from '$lib/ipc/bindings';
  import { KIND_LABEL_KEY, ORIGIN_LABEL_KEY } from './types';
  import type { Task } from './types';

  let { task, onClose }: { task: Task; onClose: () => void } = $props();

  type OutcomeKind = DetailOutcome['kind'];
  type OutcomeFilter = 'all' | OutcomeKind;

  // `task.details` is `null` while a task hasn't finished (or has no report
  // to show); OperationsPanel only renders the Details button once it is
  // non-null, but this stays defensive rather than assuming the caller.
  const details = $derived(task.details ?? []);
  const indexed = $derived(details.map((detail, index) => ({ detail, index })));

  let filter = $state<OutcomeFilter>('all');
  let expandedRows = $state<Set<number>>(new Set());

  function toggleRow(index: number) {
    const next = new Set(expandedRows);
    if (next.has(index)) {
      next.delete(index);
    } else {
      next.add(index);
    }
    expandedRows = next;
  }

  // Counts are always over the FULL set, not the current filter — a chip's
  // own count must not change just because a different chip is selected.
  const counts = $derived.by(() => {
    const c: Record<OutcomeKind, number> = { installed: 0, unchanged: 0, skipped: 0, failed: 0 };
    for (const d of details) c[d.outcome.kind]++;
    return c;
  });

  const filtered = $derived(
    filter === 'all' ? indexed : indexed.filter((row) => row.detail.outcome.kind === filter),
  );

  const totalBytes = $derived(details.reduce((sum, d) => sum + (d.bytes ?? 0), 0));

  const filterOptions = $derived([
    {
      value: 'all' as const,
      label: $t('tasks.report.filesCount', { count: details.length }),
      tone: 'accent' as const,
      testId: 'task-report-filter-all',
    },
    {
      value: 'installed' as const,
      label: $t('tasks.report.filter.installed', { count: counts.installed }),
      tone: 'success' as const,
      testId: 'task-report-filter-installed',
    },
    {
      value: 'unchanged' as const,
      label: $t('tasks.report.filter.unchanged', { count: counts.unchanged }),
      tone: 'muted' as const,
      testId: 'task-report-filter-unchanged',
    },
    {
      value: 'skipped' as const,
      label: $t('tasks.report.filter.skipped', { count: counts.skipped }),
      tone: 'warning' as const,
      testId: 'task-report-filter-skipped',
    },
    {
      value: 'failed' as const,
      label: $t('tasks.report.filter.failed', { count: counts.failed }),
      tone: 'danger' as const,
      testId: 'task-report-filter-failed',
    },
  ]);

  function badgeVariant(kind: OutcomeKind): BadgeVariant {
    switch (kind) {
      case 'installed':
        return 'success';
      case 'unchanged':
        return 'neutral';
      case 'skipped':
        return 'warning';
      case 'failed':
        return 'danger';
    }
  }

  function badgeLabel(kind: OutcomeKind): string {
    switch (kind) {
      case 'installed':
        return $t('tasks.report.badge.installed');
      case 'unchanged':
        return $t('tasks.report.badge.unchanged');
      case 'skipped':
        return $t('tasks.report.badge.skipped');
      case 'failed':
        return $t('tasks.report.badge.failed');
    }
  }

  // The row's secondary line. See the module doc comment: `fetched` and
  // `placement` are independent facts and every combination is spelled out
  // explicitly on purpose — no shared fallthrough that could let "linked"
  // silently stand in for "not downloaded" or vice-versa.
  function outcomeDetailText(outcome: DetailOutcome): string {
    switch (outcome.kind) {
      case 'installed':
        if (outcome.fetched === 'downloaded' && outcome.placement === 'linked') {
          return $t('tasks.report.outcome.downloadedLinked');
        }
        if (outcome.fetched === 'cached' && outcome.placement === 'linked') {
          return $t('tasks.report.outcome.cachedLinked');
        }
        if (outcome.fetched === 'downloaded' && outcome.placement === 'copied') {
          return $t('tasks.report.outcome.downloadedCopied');
        }
        return $t('tasks.report.outcome.cachedCopied');
      case 'unchanged':
        return $t('tasks.report.outcome.unchanged');
      case 'skipped':
      case 'failed':
        return $t('tasks.report.reason', { reason: outcome.reason });
    }
  }

  function reportLine(detail: TaskDetail): string {
    const parts = [detail.name, badgeLabel(detail.outcome.kind), outcomeDetailText(detail.outcome)];
    if (detail.host) parts.push(detail.host);
    if (detail.sha1) parts.push(detail.sha1);
    parts.push(detail.install_path);
    return parts.join(' — ');
  }

  async function copyReport() {
    const header = `${$t(KIND_LABEL_KEY[task.kind])} — ${task.title} (${filtered.length})`;
    const lines = filtered.map((row) => reportLine(row.detail));
    await navigator.clipboard.writeText([header, ...lines].join('\n'));
  }
</script>

<Modal
  ariaLabelledby="task-report-title"
  {onClose}
  panelClass="w-full max-w-2xl max-h-[85vh] flex flex-col"
  dataTestid="task-report-modal"
>
  <header class="shrink-0 border-b p-4">
    <h2 id="task-report-title" class="text-sm font-semibold text-primary">
      {$t('tasks.report.title')}
    </h2>
    <p class="mt-0.5 truncate text-xs text-secondary">
      {$t(KIND_LABEL_KEY[task.kind])} · {task.title}
    </p>
    <p class="mt-1 text-xs text-muted" data-testid="task-report-totals">
      {$t('tasks.report.filesCount', { count: details.length })}
      {#if totalBytes > 0}
        · {formatSize($t, totalBytes)}
      {/if}
    </p>
  </header>

  {#if details.length > 0}
    <div class="shrink-0 border-b px-4 py-2">
      <ToggleChipGroup
        options={filterOptions}
        value={filter}
        ariaLabel={$t('tasks.report.filterAriaLabel')}
        onChange={(v) => (filter = v as OutcomeFilter)}
      />
    </div>
  {/if}

  <div class="min-h-0 flex-1 overflow-y-auto px-4 py-2">
    {#if details.length === 0}
      <p class="p-4 text-sm text-muted">{$t('tasks.report.empty')}</p>
    {:else if filtered.length === 0}
      <p class="p-4 text-sm text-muted">{$t('tasks.report.emptyFiltered')}</p>
    {:else}
      <ul class="flex flex-col gap-1">
        {#each filtered as row (row.index)}
          {@const d = row.detail}
          {@const isOpen = expandedRows.has(row.index)}
          <li class="report-row rounded border" data-testid={`task-report-row-${row.index}`}>
            <button
              type="button"
              class="btn-tertiary flex w-full items-start gap-2 px-2 py-1.5 text-left"
              aria-expanded={isOpen}
              onclick={() => toggleRow(row.index)}
            >
              <Icon
                name={isOpen ? 'chevronDown' : 'caret'}
                size={14}
                class="mt-0.5 shrink-0 text-muted"
              />
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="truncate font-mono text-xs text-primary">{d.name}</span>
                  <span class="shrink-0 text-[10px] uppercase tracking-wide text-muted">
                    {$t(ORIGIN_LABEL_KEY[d.origin])}
                  </span>
                  <StatusBadge variant={badgeVariant(d.outcome.kind)}
                    >{badgeLabel(d.outcome.kind)}</StatusBadge
                  >
                  {#if d.bytes}
                    <span class="ml-auto shrink-0 text-xs text-muted"
                      >{formatSize($t, d.bytes)}</span
                    >
                  {/if}
                </div>
                <div class="truncate text-xs text-secondary">{outcomeDetailText(d.outcome)}</div>
              </div>
            </button>
            {#if isOpen}
              <div
                class="selectable space-y-1 border-t px-2 py-2 text-xs"
                data-testid={`task-report-row-detail-${row.index}`}
              >
                {#if d.host}
                  <div>
                    <span class="text-muted">{$t('tasks.report.detail.host')}:</span>
                    {d.host}
                  </div>
                {/if}
                {#if d.sha1}
                  <div class="break-all">
                    <span class="text-muted">{$t('tasks.report.detail.sha1')}:</span>
                    {d.sha1}
                  </div>
                {/if}
                <div class="break-all">
                  <span class="text-muted">{$t('tasks.report.detail.path')}:</span>
                  {d.install_path}
                </div>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <footer class="shrink-0 flex items-center justify-end gap-2 border-t p-3">
    <button
      type="button"
      class="btn-secondary btn-sm"
      data-testid="task-report-copy"
      disabled={filtered.length === 0}
      onclick={() => void copyReport()}
    >
      {$t('tasks.report.copyReport')}
    </button>
    <button type="button" class="btn-secondary btn-sm" onclick={onClose}>
      {$t('common.close')}
    </button>
  </footer>
</Modal>

<style>
  /* Same content-visibility idiom as LogsPopover's `.log-row` (see that
     component's <style> block for the full rationale): the browser skips
     layout/paint for rows outside the viewport, so a report with hundreds
     of files scrolls smoothly without hand-rolled virtualisation, while
     rows stay in the DOM so filtering keeps working unchanged. The
     `contain-intrinsic-size` estimate is a collapsed row's typical height
     (two text lines + padding); `auto` lets the browser remember the real
     size once a row has been rendered, including after it is expanded. */
  .report-row {
    content-visibility: auto;
    contain-intrinsic-size: auto 3.25em;
  }
</style>

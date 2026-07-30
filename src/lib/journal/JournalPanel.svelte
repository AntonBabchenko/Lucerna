<script lang="ts">
  // The instance-history view inside the Logs modal: a newest-first, day-grouped
  // list of content changes and finished launch attempts, read from the
  // per-instance append-only journal. Read-only — no row mutates anything; the
  // only action is clearing the local record.
  import { commands, type JournalEntry } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { relativeDate } from '$lib/format/relative-time';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import ToggleChipGroup from '$lib/ui/ToggleChipGroup.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import JournalRow from './JournalRow.svelte';
  import { groupByDay, journalFilter, type JournalFilterMode } from './journal-view';

  let {
    instanceId,
    onOpenLog = null,
  }: {
    instanceId: string | null;
    /** Selects a log file back in the Logs view (launch rows deep-link). */
    onOpenLog?: ((path: string) => void) | null;
  } = $props();

  let entries = $state<JournalEntry[]>([]);
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let filter = $state<JournalFilterMode>('all');
  let clearing = $state(false);
  let confirmClear = $state(false);

  // 0 lets the backend apply its own default page size.
  const READ_LIMIT = 0;

  async function load(id: string) {
    loading = true;
    loadError = null;
    const res = await commands.instanceJournalRead(id, READ_LIMIT);
    // The instance switched mid-await: drop this stale response, but do NOT
    // clear `loading` — the newer load owns it, and clearing here would flash
    // the empty state under the in-flight request.
    if (instanceId !== id) return;
    loading = false;
    if (res.status === 'ok') {
      entries = res.data;
    } else {
      entries = [];
      loadError = $t('logs.journal.loadFailed', { error: formatError(res.error) });
    }
  }

  $effect(() => {
    const id = instanceId;
    if (!id) {
      entries = [];
      loadError = null;
      loading = false;
      return;
    }
    void load(id);
  });

  async function doClear() {
    const id = instanceId;
    if (!id) return;
    clearing = true;
    const res = await commands.instanceJournalClear(id);
    clearing = false;
    if (res.status === 'ok') {
      confirmClear = false;
      entries = [];
      pushSuccess($t('logs.journal.cleared'));
    } else {
      loadError = formatError(res.error);
      confirmClear = false;
    }
  }

  const visible = $derived(journalFilter(entries, filter));
  const groups = $derived(groupByDay(visible));

  const filterOptions = $derived([
    {
      value: 'all',
      label: $t('logs.journal.filterAll'),
      tone: 'accent' as const,
      count: entries.length,
      testId: 'journal-filter-all',
    },
    {
      value: 'content',
      label: $t('logs.journal.filterContent'),
      tone: 'accent' as const,
      count: entries.filter((e) => e.event.kind === 'content').length,
      testId: 'journal-filter-content',
    },
    {
      value: 'launches',
      label: $t('logs.journal.filterLaunches'),
      tone: 'accent' as const,
      count: entries.filter((e) => e.event.kind === 'launch').length,
      testId: 'journal-filter-launches',
    },
  ]);
</script>

<section class="flex-1 flex flex-col overflow-hidden" data-testid="journal-panel">
  <div class="flex items-center gap-3 px-4 py-2 border-b flex-wrap">
    <ToggleChipGroup
      options={filterOptions}
      value={filter}
      ariaLabel={$t('logs.journal.title')}
      onChange={(v) => (filter = v as JournalFilterMode)}
    />
    <div class="flex-1"></div>
    <button
      type="button"
      class="btn-icon btn-icon-danger"
      data-testid="journal-clear"
      aria-label={$t('logs.journal.clear')}
      use:tooltip={$t('logs.journal.clear')}
      disabled={entries.length === 0 || !instanceId}
      onclick={() => (confirmClear = true)}
    >
      <Icon name="eraser" />
    </button>
  </div>

  {#if loading}
    <div class="flex justify-center p-4 text-secondary">
      <Spinner delayMs={150} label={$t('logs.journal.title')} />
    </div>
  {:else if loadError}
    <p class="p-4 text-sm text-danger" data-testid="journal-error">{loadError}</p>
  {:else if groups.length === 0}
    <p class="p-4 text-sm text-muted" data-testid="journal-empty">{$t('logs.journal.empty')}</p>
  {:else}
    <div class="flex-1 overflow-y-auto">
      {#each groups as group (group.key)}
        <h3
          class="sticky top-0 z-10 bg-subtle px-4 py-1 text-xs font-semibold text-muted border-b border-border-subtle"
        >
          {relativeDate($t, group.dayStartMs)}
        </h3>
        <ul>
          {#each group.entries as entry, i (`${group.key}:${i}`)}
            <li class="border-b border-border-subtle/50 last:border-0">
              <JournalRow {entry} {onOpenLog} />
            </li>
          {/each}
        </ul>
      {/each}
    </div>
  {/if}
</section>

{#if confirmClear}
  <ConfirmDialog
    title={$t('logs.journal.clearTitle')}
    bodyText={$t('logs.journal.clearBody')}
    confirmLabel={$t('logs.journal.clearConfirm')}
    confirmTestid="journal-clear-confirm"
    variant="danger"
    busy={clearing}
    onCancel={() => (confirmClear = false)}
    onConfirm={() => void doClear()}
  />
{/if}

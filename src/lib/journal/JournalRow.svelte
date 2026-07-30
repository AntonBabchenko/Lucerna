<script lang="ts">
  // One journal row. Two shapes behind one layout: a content change (icon,
  // action label, subject, version fragment) and a finished launch (icon,
  // outcome label, duration, exit code). Everything user-visible resolves
  // through i18n; only versions, names and numbers come from the backend.
  import type { JournalEntry } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { formatDuration } from '$lib/format/duration';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { ACTION_COPY, entryTime, OUTCOME_COPY, toneClass, versionLabel } from './journal-view';

  let {
    entry,
    onOpenLog = null,
  }: {
    entry: JournalEntry;
    /** Provided by the panel for launch rows that captured a log. */
    onOpenLog?: ((path: string) => void) | null;
  } = $props();

  const event = $derived(entry.event);
  const copy = $derived(
    event.kind === 'content' ? ACTION_COPY[event.action] : OUTCOME_COPY[event.outcome],
  );
  const clockLabel = $derived(
    new Date(entryTime(entry)).toLocaleTimeString(undefined, {
      hour: '2-digit',
      minute: '2-digit',
    }),
  );
</script>

<div class="flex items-baseline gap-2 px-3 py-1.5 text-sm" data-testid="journal-row">
  <span class="shrink-0 text-xs text-placeholder tabular-nums w-10">{clockLabel}</span>
  <span class="shrink-0 {toneClass(copy.tone)}"><Icon name={copy.icon} size={14} /></span>
  <span class="shrink-0 text-secondary">{$t(copy.labelKey)}</span>

  {#if event.kind === 'content'}
    {#if event.subject}
      <span class="min-w-0 truncate font-medium text-primary" title={event.subject}
        >{event.subject}</span
      >
    {/if}
    {@const version = versionLabel(event.from_version, event.to_version)}
    {#if version}
      <span class="shrink-0 font-mono text-xs text-muted">{version}</span>
    {/if}
    {#if event.affected !== null && event.affected > 1}
      <span class="shrink-0 text-xs text-muted"
        >{$t('logs.journal.affected', { count: event.affected })}</span
      >
    {/if}
  {:else}
    <span class="shrink-0 text-xs text-muted">{formatDuration($t, event.duration_seconds)}</span>
    {#if event.outcome === 'crashed' && event.exit_code !== null}
      <span class="shrink-0 font-mono text-xs text-muted"
        >{$t('logs.journal.exitCode', { code: event.exit_code })}</span
      >
    {/if}
    {#if onOpenLog && event.log_path}
      {@const path = event.log_path}
      <button
        type="button"
        class="btn-tertiary btn-xs shrink-0"
        use:tooltip={$t('logs.journal.openLog')}
        onclick={() => onOpenLog?.(path)}
      >
        {$t('logs.journal.openLog')}
      </button>
    {/if}
  {/if}
</div>

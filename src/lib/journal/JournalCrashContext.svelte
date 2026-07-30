<script lang="ts">
  // The crash-correlation strip. When a log carries a diagnosis (or IS a crash
  // report), this answers the question the diagnosis cannot: "did I change
  // something right before this?". It lists the content changes recorded in the
  // 24 h leading up to the log, newest first.
  //
  // Renders nothing when there is nothing to correlate — an empty strip would be
  // noise above every log.
  import { commands, type JournalEntry } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { formatDuration } from '$lib/format/duration';
  import { Icon } from '$lib/ui/icons';
  import {
    ACTION_COPY,
    contentChangesBefore,
    entryTime,
    toneClass,
    versionLabel,
  } from './journal-view';

  let {
    instanceId,
    anchorMs,
  }: {
    instanceId: string | null;
    /** Modification time of the log being read. `null` disables the strip. */
    anchorMs: number | null;
  } = $props();

  /** Changes older than this are not plausibly related to the run. */
  const WINDOW_MS = 24 * 60 * 60 * 1000;
  const MAX_ROWS = 3;
  const READ_LIMIT = 0;

  let entries = $state<JournalEntry[]>([]);

  $effect(() => {
    const id = instanceId;
    if (!id) {
      entries = [];
      return;
    }
    void (async () => {
      const res = await commands.instanceJournalRead(id, READ_LIMIT);
      // A journal read failure degrades to "no context" — this is supporting
      // evidence beside a diagnosis, never worth its own error banner.
      if (instanceId !== id) return;
      entries = res.status === 'ok' ? res.data : [];
    })();
  });

  const nearby = $derived(
    anchorMs === null ? [] : contentChangesBefore(entries, anchorMs, WINDOW_MS, MAX_ROWS),
  );
</script>

{#if nearby.length > 0 && anchorMs !== null}
  <div
    class="mx-3 mt-3 shrink-0 rounded border border-border-subtle bg-subtle p-2"
    data-testid="journal-crash-context"
  >
    <p class="flex items-center gap-1.5 text-xs font-semibold text-secondary">
      <Icon name="scrollText" size={14} />
      {$t('logs.journal.contextTitle')}
    </p>
    <ul class="mt-1 space-y-0.5">
      {#each nearby as entry, i (`${entryTime(entry)}:${i}`)}
        {#if entry.event.kind === 'content'}
          {@const copy = ACTION_COPY[entry.event.action]}
          {@const version = versionLabel(entry.event.from_version, entry.event.to_version)}
          <li class="flex items-baseline gap-2 text-xs">
            <span class="shrink-0 {toneClass(copy.tone)}"><Icon name={copy.icon} size={12} /></span>
            <span class="shrink-0 text-secondary">{$t(copy.labelKey)}</span>
            {#if entry.event.subject}
              <span class="min-w-0 truncate font-medium text-primary">{entry.event.subject}</span>
            {/if}
            {#if version}
              <span class="shrink-0 font-mono text-muted">{version}</span>
            {/if}
            <span class="shrink-0 text-muted"
              >{$t('logs.journal.contextAgo', {
                duration: formatDuration($t, (anchorMs - entryTime(entry)) / 1000),
              })}</span
            >
          </li>
        {/if}
      {/each}
    </ul>
  </div>
{/if}

<script lang="ts">
  // Step 1 of the version switch: the pack's published versions, newest first,
  // each row a selection control. A mature pack has hundreds of versions and the
  // real user task is "find the one for my Minecraft version", so the Minecraft
  // filter is part of the list rather than a nicety.
  //
  // The filter uses the shared Select component rather than a native select
  // element: WebKitGTK draws the native popup as an OS widget that ignores the
  // theme tokens. (Spelling the tag out here would trip the no-native-select
  // guard, which is a plain text scan over src/.)
  import type { ModpackVersionEntry } from '$lib/ipc/bindings';
  import { locale, t } from '$lib/i18n';
  import { formatDate } from '$lib/format/date-time';
  import Select, { type SelectOption } from '$lib/ui/Select.svelte';
  import CardShell from '$lib/ui/cards/CardShell.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import { sortVersionsNewestFirst } from './switch-risks';

  let {
    versions,
    installedVersionId,
    onSelect,
  }: {
    versions: ModpackVersionEntry[];
    installedVersionId: string | null;
    onSelect: (entry: ModpackVersionEntry) => void;
  } = $props();

  const ALL = '';

  let mcFilter = $state<string>(ALL);

  const sorted = $derived(sortVersionsNewestFirst(versions));

  // Every Minecraft version this pack has ever targeted, in newest-list order.
  const mcOptions = $derived<SelectOption[]>([
    { value: ALL, label: $t('modpacks.switch.filterMcAll') },
    ...[...new Set(sorted.flatMap((v) => v.game_versions))].map((mc) => ({
      value: mc,
      label: mc,
    })),
  ]);

  const shown = $derived(
    mcFilter === ALL ? sorted : sorted.filter((v) => v.game_versions.includes(mcFilter)),
  );

  const installedAt = $derived(sorted.findIndex((v) => v.id === installedVersionId));

  // Precomputed so a row's position is O(1). A mature pack has hundreds of
  // versions, and an indexOf per row would make rendering quadratic.
  const indexById = $derived(new Map(sorted.map((v, i) => [v.id, i])));

  // null when the installed version cannot be placed in the list (delisted, or
  // a drag-drop import with no provenance) — claiming newer/older would be a guess.
  function relation(entry: ModpackVersionEntry): 'installed' | 'newer' | 'older' | null {
    if (installedAt === -1) return null;
    const at = indexById.get(entry.id);
    if (at === undefined) return null;
    if (at === installedAt) return 'installed';
    return at < installedAt ? 'newer' : 'older';
  }

  // The platform publishes ISO strings; an unparseable one falls back to the
  // raw text rather than rendering "Invalid Date".
  function publishedLabel(iso: string): string {
    const ms = Date.parse(iso);
    return Number.isNaN(ms) ? iso : formatDate($locale, ms);
  }
</script>

<div class="flex flex-col gap-2 min-h-0 flex-1">
  {#if mcOptions.length > 2}
    <div class="flex items-center gap-2 text-sm">
      <span class="text-secondary">{$t('modpacks.switch.filterMcLabel')}</span>
      <Select
        value={mcFilter}
        options={mcOptions}
        onChange={(v) => (mcFilter = String(v))}
        ariaLabel={$t('modpacks.switch.filterMcLabel')}
        dataTestid="version-mc-filter"
      />
    </div>
  {/if}

  <div class="flex-1 overflow-y-auto border rounded min-h-0">
    {#each shown as entry (entry.id)}
      {@const rel = relation(entry)}
      <CardShell variant="compact-row" highlighted={rel === 'installed'}>
        <button
          type="button"
          class="flex-1 min-w-0 text-left"
          onclick={() => onSelect(entry)}
          data-testid={`version-row-${entry.id}`}
        >
          <div class="flex items-center gap-2 min-w-0">
            <span class="font-medium truncate">{entry.version_number}</span>
            {#if rel === 'installed'}
              <StatusBadge variant="info">{$t('modpacks.switch.installedBadge')}</StatusBadge>
            {:else if rel === 'newer'}
              <StatusBadge variant="success">{$t('modpacks.switch.newerBadge')}</StatusBadge>
            {:else if rel === 'older'}
              <StatusBadge variant="neutral">{$t('modpacks.switch.olderBadge')}</StatusBadge>
            {/if}
          </div>
          <div class="text-xs text-muted truncate">
            {$t('modpacks.switch.versionMeta', {
              mc: entry.game_versions.join(', '),
              loader: entry.loaders.join(', '),
              date: publishedLabel(entry.date_published),
            })}
          </div>
        </button>
      </CardShell>
    {/each}
    {#if shown.length === 0}
      <div class="px-2 py-4 text-muted text-center text-sm" data-testid="version-list-empty">
        {$t('modpacks.switch.versionsEmpty')}
      </div>
    {/if}
  </div>
</div>

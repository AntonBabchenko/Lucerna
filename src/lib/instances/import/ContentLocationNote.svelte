<script lang="ts">
  import type { ContentEntry, ContentLocation } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';

  // The second line under a content row: where a linked folder points, or why
  // a folder cannot be copied. A plain folder — or no entry — needs no note.
  // `options.txt` is the one category that is a file, so it is called one.
  // A row that is not a content category (the mod-data transfer's roots)
  // passes its `location` and `testid` instead of an `entry`.
  let {
    entry = null,
    location = null,
    testid = null,
  }: {
    entry?: ContentEntry | null;
    location?: ContentLocation | null;
    testid?: string | null;
  } = $props();
  const loc = $derived(entry?.location ?? location);
  const isFile = $derived(entry?.category === 'options_txt');
  const tid = $derived(testid ?? `location-note-${entry?.category}`);
</script>

{#if loc?.kind === 'linked'}
  <span class="flex items-start gap-1 text-xs text-muted" data-testid={tid}>
    <Icon name="link" size={12} class="mt-0.5 shrink-0" />
    <span class="min-w-0 break-all"
      >{$t(isFile ? 'instances.import.linkedFile' : 'instances.import.linkedFolder')}
      <span class="font-mono">{loc.target}</span></span
    >
  </span>
{:else if loc?.kind === 'broken_link'}
  <span class="flex items-start gap-1 text-xs text-warning-text" data-testid={tid}>
    <Icon name="warning" size={12} class="mt-0.5 shrink-0" />
    <span class="min-w-0 break-all"
      >{$t(isFile ? 'instances.import.brokenLinkFile' : 'instances.import.brokenLink')}
      <span class="font-mono">{loc.target}</span></span
    >
  </span>
{:else if loc?.kind === 'unreadable'}
  <span class="flex items-start gap-1 text-xs text-warning-text" data-testid={tid}>
    <Icon name="warning" size={12} class="mt-0.5 shrink-0" />
    <span class="min-w-0 break-all"
      >{$t(isFile ? 'instances.import.unreadableFile' : 'instances.import.unreadableFolder', {
        reason: loc.reason,
      })}</span
    >
  </span>
{/if}

<script lang="ts">
  import type { ContentEntry } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';

  // The second line under a content row: where a linked folder points, or why
  // a folder cannot be copied. A plain folder — or no entry — needs no note.
  let { entry }: { entry: ContentEntry | null } = $props();
  const loc = $derived(entry?.location ?? null);
</script>

{#if loc?.kind === 'linked'}
  <span
    class="flex items-start gap-1 text-xs text-muted"
    data-testid={`location-note-${entry?.category}`}
  >
    <Icon name="link" size={12} class="mt-0.5 shrink-0" />
    <span class="min-w-0 break-all"
      >{$t('instances.import.linkedFolder')} <span class="font-mono">{loc.target}</span></span
    >
  </span>
{:else if loc?.kind === 'broken_link'}
  <span
    class="flex items-start gap-1 text-xs text-warning-text"
    data-testid={`location-note-${entry?.category}`}
  >
    <Icon name="warning" size={12} class="mt-0.5 shrink-0" />
    <span class="min-w-0 break-all"
      >{$t('instances.import.brokenLink')} <span class="font-mono">{loc.target}</span></span
    >
  </span>
{:else if loc?.kind === 'unreadable'}
  <span
    class="flex items-start gap-1 text-xs text-warning-text"
    data-testid={`location-note-${entry?.category}`}
  >
    <Icon name="warning" size={12} class="mt-0.5 shrink-0" />
    <span class="min-w-0 break-all"
      >{$t('instances.import.unreadableFolder', { reason: loc.reason })}</span
    >
  </span>
{/if}

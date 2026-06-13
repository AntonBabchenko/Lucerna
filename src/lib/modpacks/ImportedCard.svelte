<script lang="ts">
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';
  import { t } from '$lib/i18n';
  import { relativeDate } from '$lib/format/relative-time';
  import CardMedia from '$lib/ui/cards/CardMedia.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import { accentStripClass, type CardAccent } from '$lib/ui/cards/card-status';
  import { modpackUpdates } from './modpack-updates.svelte';

  // One card in the Imported tab grid. Mirrors the shared card language: a left
  // accent strip signals attention (update available > modified), the avatar is
  // the package placeholder (fetching real pack icons per render isn't worth it),
  // and tags use the shared StatusBadge.
  let {
    inst,
    onClick,
    isModified = false,
  }: { inst: InstanceWithStatus; onClick: () => void; isModified?: boolean } = $props();

  const updateEntry = $derived.by(() => {
    const s = modpackUpdates.statusFor(inst.id);
    return s?.kind === 'update_available' ? s.entry : null;
  });

  // Attention accent: update available (success) > modified (warning) > none.
  const accent = $derived<CardAccent>(updateEntry ? 'success' : isModified ? 'warning' : 'none');
</script>

<button
  type="button"
  class="relative overflow-hidden text-left p-3 pl-4 bg-surface border border-border-subtle rounded-lg hover:border-accent hover:bg-subtle transition-colors w-full"
  onclick={onClick}
  data-testid="imported-card"
>
  <span
    aria-hidden="true"
    class={`absolute left-0 top-0 bottom-0 w-[3px] ${accentStripClass(accent)}`}
  ></span>
  <div class="flex gap-3">
    <CardMedia iconUrl={null} placeholder="package" size="lg" />
    <div class="min-w-0 flex-1">
      <div class="font-semibold text-sm truncate flex items-center gap-1.5 flex-wrap">
        <span>{inst.mrpack_name} v{inst.mrpack_version}</span>
        {#if isModified}
          <span data-testid="imported-card-modified-tag" title={$t('modpacks.imported.card.modifiedTitle')}>
            <StatusBadge variant="warning" icon="info"
              >{$t('modpacks.imported.card.modifiedTag')}</StatusBadge
            >
          </span>
        {/if}
        {#if updateEntry}
          <span data-testid="imported-card-update-tag" title={$t('modpacks.imported.card.updateTitle')}>
            <StatusBadge variant="success"
              >{$t('modpacks.imported.card.updateTag', { version: updateEntry.version_number })}</StatusBadge
            >
          </span>
        {/if}
      </div>
      <div class="text-xs text-muted truncate">
        {$t('modpacks.imported.card.instanceLabel', { name: inst.name })}
      </div>
      <div class="text-xs text-placeholder mt-1 truncate">
        MC {inst.mc_version} · {displayLoader(inst.loader)}{inst.loader_version
          ? ' ' + inst.loader_version
          : ''}{inst.created_unix_ms != null
          ? ' · ' +
            $t('modpacks.imported.card.importedAt', {
              when: relativeDate($t, inst.created_unix_ms),
            })
          : ''}
      </div>
    </div>
  </div>
</button>

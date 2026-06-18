<script lang="ts">
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import { accentStripClass } from '$lib/ui/cards/card-status';
  import BusyButton from '$lib/ui/BusyButton.svelte';

  let { instance, onOpenPack }: { instance: InstanceWithStatus; onOpenPack: () => void } = $props();

  // Brand source labels are proper nouns — same in every locale.
  const SOURCE_LABEL: Record<string, string> = {
    modrinth: 'Modrinth',
    curseforge: 'CurseForge',
    ftb: 'FTB',
    atlauncher: 'ATLauncher',
  };

  const sourceLabel = $derived(
    instance.mrpack_source ? (SOURCE_LABEL[instance.mrpack_source] ?? instance.mrpack_source) : '',
  );

  // Reads the shared store: a background sweep (startup / Imported tab) may
  // already have resolved this instance's status, so the card reflects it
  // without a manual click. The button below force-refreshes via the store.
  const status = $derived(modpackUpdates.statusFor(instance.id));
  let checking = $state(false);

  async function onCheck() {
    checking = true;
    await modpackUpdates.sweep([instance.id], { force: true });
    checking = false;
  }
</script>

<div
  class="relative overflow-hidden rounded-xl border border-border-subtle bg-surface p-3.5 pl-4 flex flex-col gap-2.5"
  data-testid="overview-modpack-card"
>
  <span
    aria-hidden="true"
    class={`absolute left-0 top-0 bottom-0 w-[3px] ${accentStripClass(status?.kind === 'update_available' ? 'success' : 'none')}`}
  ></span>
  <div class="text-[10px] uppercase tracking-wider text-muted">
    {$t('page.overview.sectionModpack')}
  </div>

  <div class="flex items-center gap-2.5 flex-wrap">
    <span class="font-semibold text-primary">{instance.mrpack_name}</span>
    {#if instance.mrpack_version}
      <span class="text-secondary text-sm">v{instance.mrpack_version}</span>
    {/if}
    {#if sourceLabel}
      <span
        class="rounded-full border border-border-subtle bg-base px-2 py-0.5 text-xs text-secondary"
        >{sourceLabel}</span
      >
    {/if}
    {#if status?.kind === 'update_available'}
      <span data-testid="modpack-update-available">
        <StatusBadge variant="success"
          >{$t('page.overview.modpackUpdateAvailable', {
            version: status.entry.version_number,
          })}</StatusBadge
        >
      </span>
    {/if}
    <button
      type="button"
      class="btn-tertiary ml-auto inline-flex items-center gap-1"
      onclick={onOpenPack}
    >
      {$t('page.overview.modpackOpen')}
      <Icon name="arrowRight" size={14} />
    </button>
  </div>

  {#if instance.mrpack_summary}
    <p class="text-xs text-muted">{instance.mrpack_summary}</p>
  {/if}

  <div class="flex items-center gap-3">
    <BusyButton
      type="button"
      class="btn-secondary btn-sm"
      data-testid="modpack-check-update"
      busy={checking}
      onclick={onCheck}
    >
      {checking ? $t('page.overview.modpackChecking') : $t('page.overview.modpackCheckUpdate')}
    </BusyButton>
    {#if status?.kind === 'up_to_date'}
      <span class="text-xs text-success" data-testid="modpack-up-to-date"
        >{$t('page.overview.modpackUpToDate')}</span
      >
    {:else if status?.kind === 'check_failed'}
      <span class="text-xs text-danger" data-testid="modpack-update-error"
        >{$t('page.overview.modpackCheckFailed')}</span
      >
    {:else if status?.kind === 'not_checkable'}
      <span class="text-xs text-muted" data-testid="modpack-not-checkable">
        {#if status.reason === 'needs_curseforge_key'}
          {$t('page.overview.modpackNotCheckableNeedsKey')}
        {:else}
          {$t('page.overview.modpackNotCheckableNoProvenance')}
        {/if}
      </span>
    {/if}
  </div>
</div>

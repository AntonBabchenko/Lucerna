<script lang="ts">
  import { commands, type InstanceWithStatus, type ModpackVersionEntry } from '$lib/ipc/bindings';
  import type { Error as IpcError } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';

  let { instance, onOpenPack }: { instance: InstanceWithStatus; onOpenPack: () => void } = $props();

  // Brand source labels are proper nouns — same in every locale.
  const SOURCE_LABEL: Record<string, string> = {
    modrinth: 'Modrinth',
    curseforge: 'CurseForge',
    ftb: 'FTB',
  };

  const sourceLabel = $derived(
    instance.mrpack_source ? (SOURCE_LABEL[instance.mrpack_source] ?? instance.mrpack_source) : '',
  );

  const isModrinth = $derived(instance.mrpack_source === 'modrinth');

  type CheckState =
    | { phase: 'idle' }
    | { phase: 'checking' }
    | { phase: 'available'; entry: ModpackVersionEntry }
    | { phase: 'up_to_date' }
    | { phase: 'error'; message: string };

  let check = $state<CheckState>({ phase: 'idle' });

  async function onCheck() {
    check = { phase: 'checking' };
    const r = await commands.modpackCheckUpdate(instance.id);
    if (r.status === 'error') {
      check = { phase: 'error', message: formatError(r.error as IpcError) };
      return;
    }
    check = r.data ? { phase: 'available', entry: r.data } : { phase: 'up_to_date' };
  }
</script>

<div
  class="rounded-xl border border-border-subtle bg-surface p-3.5 flex flex-col gap-2.5"
  data-testid="overview-modpack-card"
>
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
    {#if check.phase === 'available'}
      <span
        class="rounded-full border border-success bg-success-bg px-2 py-0.5 text-xs text-success"
        data-testid="modpack-update-available"
      >
        {$t('page.overview.modpackUpdateAvailable', { version: check.entry.version_number })}
      </span>
    {/if}
    <button type="button" class="btn-tertiary ml-auto" onclick={onOpenPack}>
      {$t('page.overview.modpackOpen')}
    </button>
  </div>

  {#if instance.mrpack_summary}
    <p class="text-xs text-muted">{instance.mrpack_summary}</p>
  {/if}

  {#if isModrinth}
    <div class="flex items-center gap-3">
      <button
        type="button"
        class="btn-secondary btn-sm"
        data-testid="modpack-check-update"
        disabled={check.phase === 'checking'}
        onclick={onCheck}
      >
        {check.phase === 'checking'
          ? $t('page.overview.modpackChecking')
          : $t('page.overview.modpackCheckUpdate')}
      </button>
      {#if check.phase === 'up_to_date'}
        <span class="text-xs text-success" data-testid="modpack-up-to-date"
          >{$t('page.overview.modpackUpToDate')}</span
        >
      {:else if check.phase === 'error'}
        <span class="text-xs text-danger" data-testid="modpack-update-error">{check.message}</span>
      {/if}
    </div>
  {:else}
    <p class="text-xs text-muted" data-testid="modpack-only-modrinth">
      {$t('page.overview.modpackOnlyModrinth')}
    </p>
  {/if}
</div>

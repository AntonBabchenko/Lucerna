<script lang="ts">
  import { Channel } from '@tauri-apps/api/core';
  import { save } from '@tauri-apps/plugin-dialog';
  import { commands } from '$lib/ipc/bindings';
  import type {
    ExportMetadata,
    ExportOptions,
    ExportPreview,
    ModpackExportProgress,
    ModpackFormat,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { formatSize } from '$lib/format/size';
  import Modal from '$lib/ui/Modal.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { defaultExportFilename, unresolvableMods, type ExportModeUi } from '$lib/modpacks/export';

  let {
    instanceId,
    instanceName,
    onClose,
  }: { instanceId: string; instanceName: string; onClose: () => void } = $props();

  let preview = $state<ExportPreview | null>(null);
  let loadError = $state<string | null>(null);
  let busy = $state(false);
  let phase = $state<ModpackExportProgress | null>(null);

  let format = $state<ModpackFormat>('modrinth');
  let mode = $state<ExportModeUi>('lightweight');
  let includeConfig = $state(true);
  let includeResourcepacks = $state(true);
  let includeShaderpacks = $state(true);
  let includeWorlds = $state(false);
  // svelte-ignore state_referenced_locally
  let name = $state(instanceName);
  let version = $state('1.0.0');
  let author = $state('');
  let summary = $state('');
  let bundleSet = $state<Set<string>>(new Set());

  $effect(() => {
    void (async () => {
      const r = await commands.exportPreview(instanceId);
      if (r.status === 'ok') {
        preview = r.data;
      } else {
        loadError = formatError(r.error);
      }
    })();
  });

  const unresolvable = $derived(preview ? unresolvableMods(preview.mods, format, mode) : []);

  $effect(() => {
    const next = new Set<string>();
    for (const m of unresolvable) next.add(m.sha1);
    bundleSet = next;
  });

  function toggleBundle(sha1: string) {
    const next = new Set(bundleSet);
    if (next.has(sha1)) next.delete(sha1);
    else next.add(sha1);
    bundleSet = next;
  }

  // Translated label for the current export phase. The backend emits a raw
  // enum (resolving/bundling/writing/done); map it to a localized string so
  // the progress line re-renders on a live locale switch.
  const phaseLabel = $derived(phase ? $t(`modpacks.export.phase.${phase.phase}`) : '');

  async function runExport() {
    const dest = await save({
      defaultPath: defaultExportFilename(name, version, format),
      filters: [
        format === 'modrinth'
          ? { name: $t('common.fileFilter.modrinthModpack'), extensions: ['mrpack'] }
          : { name: $t('common.fileFilter.curseforgeModpack'), extensions: ['zip'] },
      ],
    });
    if (!dest) return;

    busy = true;
    phase = null;
    const ch = new Channel<ModpackExportProgress>();
    ch.onmessage = (m) => {
      phase = m;
    };

    const metadata: ExportMetadata = { name, version, author, summary };
    const options: ExportOptions = {
      format,
      mode: mode === 'full' ? 'full' : 'lightweight',
      include_config: includeConfig,
      include_resourcepacks: includeResourcepacks,
      include_shaderpacks: includeShaderpacks,
      include_worlds: includeWorlds,
      bundle_shas: mode === 'full' ? [] : [...bundleSet],
      metadata,
    };

    const r = await commands.exportModpack(instanceId, options, dest, ch);
    busy = false;
    if (r.status === 'ok') {
      pushSuccess($t('modpacks.export.exported', { name, dest }));
      onClose();
    } else {
      pushWarning($t('modpacks.export.failed'), [formatError(r.error)]);
    }
  }
</script>

<Modal
  ariaLabelledby="export-pack-title"
  {onClose}
  panelClass="max-w-2xl w-full max-h-[85vh] flex flex-col"
  closeOnBackdrop={!busy}
  closeOnEscape={!busy}
>
  <header class="p-4 border-b">
    <h2 id="export-pack-title" class="text-lg font-semibold text-primary">
      {$t('modpacks.export.title')}
    </h2>
  </header>

  <div class="flex-1 overflow-y-auto p-4 flex flex-col gap-4">
    {#if loadError}
      <p class="text-sm text-danger">{loadError}</p>
    {:else if !preview}
      <LoadingPanel label={$t('modpacks.export.loading')} />
    {:else}
      <fieldset class="flex flex-col gap-1">
        <legend class="text-xs uppercase tracking-wide text-muted"
          >{$t('modpacks.export.formatLabel')}</legend
        >
        <label
          ><input type="radio" bind:group={format} value="modrinth" />
          {$t('modpacks.export.formatModrinth')}</label
        >
        <label
          ><input type="radio" bind:group={format} value="curseforge" />
          {$t('modpacks.export.formatCurseForge')}</label
        >
      </fieldset>

      <fieldset class="flex flex-col gap-1">
        <legend class="text-xs uppercase tracking-wide text-muted"
          >{$t('modpacks.export.modeLabel')}</legend
        >
        <label
          ><input type="radio" bind:group={mode} value="lightweight" />
          {$t('modpacks.export.modeLightweight')}</label
        >
        <label
          ><input type="radio" bind:group={mode} value="full" />
          {$t('modpacks.export.modeFull')}</label
        >
      </fieldset>

      <fieldset class="flex flex-col gap-1">
        <legend class="text-xs uppercase tracking-wide text-muted"
          >{$t('modpacks.export.contentLabel')}</legend
        >
        <label
          ><input type="checkbox" checked disabled />
          {$t('modpacks.export.modsCount', { count: preview.mods.length })}</label
        >
        {#if preview.has_config}
          <label
            ><input type="checkbox" bind:checked={includeConfig} />
            {$t('modpacks.export.configs')}</label
          >
          <p class="text-xs text-muted">
            {$t('modpacks.export.configsWarning')}
          </p>
        {/if}
        {#if preview.has_resourcepacks}
          <label
            ><input type="checkbox" bind:checked={includeResourcepacks} />
            {$t('modpacks.export.resourcePacks')}</label
          >
        {/if}
        {#if preview.has_shaderpacks}
          <label
            ><input type="checkbox" bind:checked={includeShaderpacks} />
            {$t('modpacks.export.shaderPacks')}</label
          >
        {/if}
        {#if preview.has_saves}
          <label>
            <input type="checkbox" bind:checked={includeWorlds} />
            {$t('modpacks.export.worlds')}{formatSize($t, preview.saves_size_bytes)
              ? ` (${formatSize($t, preview.saves_size_bytes)})`
              : ''}
          </label>
          {#if includeWorlds}
            <p class="text-xs text-danger">
              {$t('modpacks.export.worldsWarning')}
            </p>
          {/if}
        {/if}
      </fieldset>

      {#if mode === 'lightweight' && unresolvable.length > 0}
        <fieldset class="flex flex-col gap-1">
          <legend class="text-xs uppercase tracking-wide text-muted"
            >{$t('modpacks.export.unresolvableLabel')}</legend
          >
          <p class="text-xs text-muted">
            {$t('modpacks.export.unresolvableBody')}
          </p>
          {#each unresolvable as m (m.sha1)}
            <label>
              <input
                type="checkbox"
                checked={bundleSet.has(m.sha1)}
                onchange={() => toggleBundle(m.sha1)}
              />
              {m.name}
            </label>
          {/each}
        </fieldset>
      {/if}

      <fieldset class="flex flex-col gap-2">
        <legend class="text-xs uppercase tracking-wide text-muted"
          >{$t('modpacks.export.detailsLabel')}</legend
        >
        <input
          class="border rounded px-2 py-1"
          bind:value={name}
          placeholder={$t('modpacks.export.namePlaceholder')}
        />
        <input
          class="border rounded px-2 py-1"
          bind:value={version}
          placeholder={$t('modpacks.export.versionPlaceholder')}
        />
        <input
          class="border rounded px-2 py-1"
          bind:value={author}
          placeholder={$t('modpacks.export.authorPlaceholder')}
        />
        <input
          class="border rounded px-2 py-1"
          bind:value={summary}
          placeholder={$t('modpacks.export.summaryPlaceholder')}
        />
      </fieldset>

      {#if phase}
        <p class="text-sm text-secondary flex items-center gap-2" data-testid="export-phase">
          <Spinner size="sm" labelPlacement="right" label={phaseLabel} />
        </p>
      {/if}
    {/if}
  </div>

  <footer class="p-4 border-t flex justify-end gap-2">
    <button type="button" class="btn-secondary btn-sm" onclick={onClose} disabled={busy}
      >{$t('common.cancel')}</button
    >
    <button
      type="button"
      class="btn-primary btn-sm"
      disabled={busy || !preview || preview.mods.length === 0 || !name.trim() || !version.trim()}
      onclick={() => void runExport()}
    >
      {$t('modpacks.export.exportBtn')}
    </button>
  </footer>
</Modal>

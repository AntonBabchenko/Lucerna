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

  function fmtSize(bytes: number): string {
    if (bytes > 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
    return `${Math.max(1, Math.round(bytes / 1024 / 1024))} MB`;
  }

  async function runExport() {
    const dest = await save({
      defaultPath: defaultExportFilename(name, version, format),
      filters: [
        format === 'modrinth'
          ? { name: 'Modrinth modpack', extensions: ['mrpack'] }
          : { name: 'CurseForge modpack', extensions: ['zip'] },
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
      pushSuccess(`Exported ${name} to ${dest}`);
      onClose();
    } else {
      pushWarning('Export failed', [formatError(r.error)]);
    }
  }
</script>

<div
  class="fixed inset-0 z-50 bg-black/40 flex items-center justify-center"
  role="dialog"
  aria-modal="true"
  aria-label="Export modpack"
>
  <div class="bg-surface rounded-lg shadow-xl max-w-2xl w-full max-h-[85vh] flex flex-col">
    <header class="p-4 border-b">
      <h2 class="text-lg font-semibold text-primary">Export modpack</h2>
    </header>

    <div class="flex-1 overflow-y-auto p-4 flex flex-col gap-4">
      {#if loadError}
        <p class="text-sm text-danger">{loadError}</p>
      {:else if !preview}
        <p class="text-sm text-muted">Loading…</p>
      {:else}
        <fieldset class="flex flex-col gap-1">
          <legend class="text-xs uppercase tracking-wide text-muted">Format</legend>
          <label
            ><input type="radio" bind:group={format} value="modrinth" /> .mrpack (Modrinth)</label
          >
          <label
            ><input type="radio" bind:group={format} value="curseforge" /> CurseForge .zip</label
          >
        </fieldset>

        <fieldset class="flex flex-col gap-1">
          <legend class="text-xs uppercase tracking-wide text-muted">Mode</legend>
          <label
            ><input type="radio" bind:group={mode} value="lightweight" /> Lightweight (download links)</label
          >
          <label
            ><input type="radio" bind:group={mode} value="full" /> Full (bundle everything)</label
          >
        </fieldset>

        <fieldset class="flex flex-col gap-1">
          <legend class="text-xs uppercase tracking-wide text-muted">Content</legend>
          <label><input type="checkbox" checked disabled /> Mods ({preview.mods.length})</label>
          {#if preview.has_config}
            <label><input type="checkbox" bind:checked={includeConfig} /> Configs</label>
          {/if}
          {#if preview.has_resourcepacks}
            <label
              ><input type="checkbox" bind:checked={includeResourcepacks} /> Resource packs</label
            >
          {/if}
          {#if preview.has_shaderpacks}
            <label><input type="checkbox" bind:checked={includeShaderpacks} /> Shader packs</label>
          {/if}
          {#if preview.has_saves}
            <label>
              <input type="checkbox" bind:checked={includeWorlds} />
              Worlds{preview.saves_size_bytes != null
                ? ` (${fmtSize(preview.saves_size_bytes)})`
                : ''}
            </label>
            {#if includeWorlds}
              <p class="text-xs text-danger">
                Worlds may contain personal builds and large region files. Only include them if you
                intend to share your saves.
              </p>
            {/if}
          {/if}
        </fieldset>

        {#if mode === 'lightweight' && unresolvable.length > 0}
          <fieldset class="flex flex-col gap-1">
            <legend class="text-xs uppercase tracking-wide text-muted"
              >Mods without a download link</legend
            >
            <p class="text-xs text-muted">
              These can't be referenced in this format. Bundle them inside the file or skip them.
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
          <legend class="text-xs uppercase tracking-wide text-muted">Details</legend>
          <input class="border rounded px-2 py-1" bind:value={name} placeholder="Pack name" />
          <input class="border rounded px-2 py-1" bind:value={version} placeholder="Version" />
          <input
            class="border rounded px-2 py-1"
            bind:value={author}
            placeholder="Author (optional)"
          />
          <input
            class="border rounded px-2 py-1"
            bind:value={summary}
            placeholder="Summary (optional)"
          />
        </fieldset>

        {#if phase}
          <p class="text-sm text-secondary" data-testid="export-phase">{phase.phase}</p>
        {/if}
      {/if}
    </div>

    <footer class="p-4 border-t flex justify-end gap-2">
      <button type="button" class="btn-secondary btn-sm" onclick={onClose} disabled={busy}
        >Cancel</button
      >
      <button
        type="button"
        class="btn-primary btn-sm"
        disabled={busy || !preview || preview.mods.length === 0 || !name.trim() || !version.trim()}
        onclick={() => void runExport()}
      >
        Export
      </button>
    </footer>
  </div>
</div>

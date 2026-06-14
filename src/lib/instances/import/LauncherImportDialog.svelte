<script lang="ts">
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import type { ContentCategory, ForeignInstance, LoaderKind } from '$lib/ipc/bindings';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { formatSize } from '$lib/format/size';
  import { t } from '$lib/i18n';
  import { enqueueLauncherImport } from '$lib/ops/op-queue.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Select from '$lib/ui/Select.svelte';
  import { Icon } from '$lib/ui/icons';

  // Two-step wizard:
  //   Step 1 — discovery list (discover + browse-to-folder)
  //   Step 2 — category checkboxes + name override for the chosen instance

  let { onClose }: { onClose: () => void } = $props();

  // ── step tracking ──────────────────────────────────────────────────────────
  let step = $state<'discover' | 'configure'>('discover');
  let chosen = $state<ForeignInstance | null>(null);

  // ── step 1: discovery ──────────────────────────────────────────────────────
  let discovering = $state(false);
  let discovered = $state<ForeignInstance[]>([]);
  let discoverError = $state<string | null>(null);
  let hasDiscovered = $state(false);

  async function discover() {
    discovering = true;
    discoverError = null;
    try {
      const res = await commands.launcherImportDiscover();
      if (res.status === 'ok') {
        discovered = res.data;
        hasDiscovered = true;
      } else {
        discoverError = formatError(res.error);
      }
    } finally {
      discovering = false;
    }
  }

  async function browseFolder() {
    const path = await openFile({ directory: true });
    if (!path || typeof path !== 'string') return;
    const res = await commands.launcherImportInspectFolder(path);
    if (res.status === 'ok') {
      selectInstance(res.data);
    } else {
      discoverError = formatError(res.error);
    }
  }

  function selectInstance(inst: ForeignInstance) {
    chosen = inst;
    // Seed the category set and name
    seededFor = null; // force re-seed
    targetName = inst.name;
    step = 'configure';
  }

  // ── step 2: category selection ─────────────────────────────────────────────
  const ALL_CATEGORIES: ContentCategory[] = [
    'mods',
    'config',
    'saves',
    'resource_packs',
    'shaderpacks',
    'options_txt',
  ];

  let selected = $state<Set<ContentCategory>>(new Set());
  let seededFor: ForeignInstance | null = null;
  let targetName = $state('');

  // Generic `.minecraft` carries no version/loader metadata — the user
  // supplies them. Seeded from the chosen instance (blank for raw).
  let mcVersionInput = $state('');
  let loaderInput = $state<LoaderKind>('vanilla');
  const needsManualVersion = $derived(chosen?.source === 'raw_minecraft');
  const LOADER_OPTIONS: { value: string; label: string }[] = [
    { value: 'vanilla', label: 'Vanilla' },
    { value: 'fabric', label: 'Fabric' },
    { value: 'quilt', label: 'Quilt' },
    { value: 'forge', label: 'Forge' },
    { value: 'neoforge', label: 'NeoForge' },
  ];

  $effect.pre(() => {
    if (chosen && seededFor !== chosen) {
      seededFor = chosen;
      // Default: check all categories that have files
      const available = new Set(chosen.content.map((c) => c.category));
      selected = new Set(ALL_CATEGORIES.filter((c) => available.has(c)));
      targetName = chosen.name;
      mcVersionInput = chosen.mc_version;
      loaderInput = chosen.loader;
    }
  });

  function toggleCategory(cat: ContentCategory) {
    const next = new Set(selected);
    if (next.has(cat)) next.delete(cat);
    else next.add(cat);
    selected = next;
  }

  const availableCategories = $derived(
    chosen ? chosen.content.map((c) => c.category) : ([] as ContentCategory[]),
  );

  const allSelected = $derived(
    availableCategories.length > 0 && availableCategories.every((c) => selected.has(c)),
  );

  function toggleAll() {
    selected = allSelected ? new Set() : new Set(availableCategories);
  }

  function contentEntry(cat: ContentCategory) {
    return chosen?.content.find((c) => c.category === cat) ?? null;
  }

  function categoryLabel(cat: ContentCategory): string {
    const key =
      cat === 'mods'
        ? 'instances.import.categoryMods'
        : cat === 'config'
          ? 'instances.import.categoryConfig'
          : cat === 'saves'
            ? 'instances.import.categorySaves'
            : cat === 'resource_packs'
              ? 'instances.import.categoryResourcePacks'
              : cat === 'shaderpacks'
                ? 'instances.import.categoryShaderpacks'
                : 'instances.import.categoryOptionsTxt';
    return $t(key as Parameters<typeof $t>[0]);
  }

  function sourceLabel(source: ForeignInstance['source']): string {
    const key =
      source === 'prism'
        ? 'instances.import.sourcePrism'
        : source === 'curseforge_app'
          ? 'instances.import.sourceCurseforge'
          : source === 'modrinth_app'
            ? 'instances.import.sourceModrinth'
            : source === 'atlauncher'
              ? 'instances.import.sourceAtlauncher'
              : 'instances.import.sourceRaw';
    return $t(key as Parameters<typeof $t>[0]);
  }

  let importing = $state(false);

  function doImport() {
    if (!chosen || selected.size === 0 || !targetName.trim()) return;
    if (needsManualVersion && !mcVersionInput.trim()) return;
    importing = true;
    enqueueLauncherImport(targetName.trim(), {
      foreign: chosen,
      selected: [...selected],
      targetName: targetName.trim(),
      mcVersionOverride: needsManualVersion ? mcVersionInput.trim() : null,
      loaderOverride: needsManualVersion ? loaderInput : null,
      loaderVersionOverride: null,
    });
    onClose();
  }
</script>

<Modal
  {onClose}
  ariaLabel={$t('instances.import.dialogAriaLabel')}
  dataTestid="launcher-import-dialog"
  panelClass="max-w-lg w-full"
>
  <div class="p-6">
    {#if step === 'discover'}
      <!-- Step 1: discovery -->
      <h2 class="text-lg font-semibold text-primary mb-4" id="launcher-import-heading">
        {$t('instances.import.step1Title')}
      </h2>

      <div class="flex gap-2 mb-4">
        <button
          type="button"
          class="btn btn-primary flex items-center gap-2"
          onclick={discover}
          disabled={discovering}
          data-testid="discover-btn"
        >
          <Icon name="refresh" size={16} />
          {discovering ? $t('instances.import.discovering') : $t('instances.import.discover')}
        </button>
        <button
          type="button"
          class="btn btn-secondary flex items-center gap-2"
          onclick={browseFolder}
          disabled={discovering}
          data-testid="browse-folder-btn"
        >
          <Icon name="folderOpen" size={16} />
          {$t('instances.import.browseFolder')}
        </button>
      </div>

      {#if discoverError}
        <p class="text-sm text-danger mb-3" role="alert" data-testid="discover-error">
          {discoverError}
        </p>
      {/if}

      {#if hasDiscovered}
        {#if discovered.length === 0}
          <p class="text-sm text-secondary" data-testid="discover-empty">
            {$t('instances.import.discoverEmpty')}
          </p>
        {:else}
          <ul class="space-y-2 max-h-72 overflow-y-auto" data-testid="discovered-list">
            {#each discovered as inst (inst.root)}
              <li>
                <button
                  type="button"
                  class="w-full text-left rounded border border-border p-3 hover:bg-subtle transition-colors"
                  onclick={() => selectInstance(inst)}
                  data-testid="instance-row"
                >
                  <div class="flex items-center justify-between">
                    <span class="font-medium text-primary truncate">{inst.name}</span>
                    <span class="text-xs text-muted ml-2 shrink-0">{sourceLabel(inst.source)}</span>
                  </div>
                  <div class="text-xs text-secondary mt-0.5">
                    {$t('instances.import.mcLabel', { version: inst.mc_version })}
                    {#if inst.loader !== 'vanilla'}
                      · {$t('instances.import.loaderLabel', {
                        loader: inst.loader,
                        version: inst.loader_version ?? '',
                      })}
                    {/if}
                  </div>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}

      <div class="flex justify-end mt-6">
        <button type="button" class="btn btn-secondary" onclick={onClose}>
          {$t('common.cancel')}
        </button>
      </div>
    {:else if step === 'configure' && chosen}
      <!-- Step 2: category + name -->
      <div class="flex items-center gap-2 mb-4">
        <button
          type="button"
          class="btn btn-ghost p-1"
          aria-label={$t('instances.import.back')}
          onclick={() => (step = 'discover')}
          data-testid="back-btn"
        >
          <Icon name="arrowLeft" size={16} />
        </button>
        <h2 class="text-lg font-semibold text-primary" id="launcher-import-heading">
          {$t('instances.import.step2Title', { name: chosen.name })}
        </h2>
      </div>

      <!-- Name input -->
      <label class="block mb-4">
        <span class="text-sm text-secondary">{$t('instances.import.nameLabel')}</span>
        <input
          type="text"
          class="mt-1 input w-full"
          bind:value={targetName}
          data-testid="name-input"
        />
      </label>

      <!-- Generic .minecraft: user supplies version + loader -->
      {#if needsManualVersion}
        <label class="block mb-4">
          <span class="text-sm text-secondary">{$t('instances.import.mcVersionInputLabel')}</span>
          <input
            type="text"
            class="mt-1 input w-full"
            bind:value={mcVersionInput}
            placeholder={$t('instances.import.mcVersionPlaceholder')}
            data-testid="mc-version-input"
          />
          <span class="mt-1 block text-xs text-muted">
            {$t('instances.import.mcVersionHint')}
          </span>
        </label>
        <div class="block mb-4">
          <span class="text-sm text-secondary">{$t('instances.import.loaderInputLabel')}</span>
          <Select
            class="mt-1 w-full"
            value={loaderInput}
            options={LOADER_OPTIONS}
            onChange={(v) => (loaderInput = v as LoaderKind)}
            ariaLabel={$t('instances.import.loaderInputLabel')}
            dataTestid="loader-select"
          />
        </div>
      {/if}

      <!-- Source folder path (so the user can clean up the original later) -->
      <div class="mb-4 text-xs text-muted">
        <span class="text-secondary">{$t('instances.import.sourcePathLabel')}:</span>
        <span class="font-mono break-all" data-testid="source-path">{chosen.root}</span>
      </div>

      <!-- Category checkboxes -->
      <div class="mb-4">
        <div class="flex items-center justify-between mb-2">
          <span class="text-sm font-medium text-secondary">
            {$t('instances.import.contentLabel')}
          </span>
          <button
            type="button"
            class="text-xs text-accent hover:underline"
            onclick={toggleAll}
            data-testid="toggle-all-btn"
          >
            {allSelected ? $t('instances.import.deselectAll') : $t('instances.import.selectAll')}
          </button>
        </div>

        <ul class="space-y-1" data-testid="category-list">
          {#each availableCategories as cat (cat)}
            {@const entry = contentEntry(cat)}
            <li class="flex items-center gap-2">
              <input
                type="checkbox"
                id={`cat-${cat}`}
                checked={selected.has(cat)}
                onchange={() => toggleCategory(cat)}
                class="rounded"
                data-testid={`cat-${cat}`}
              />
              <label for={`cat-${cat}`} class="flex-1 text-sm text-primary cursor-pointer">
                {categoryLabel(cat)}
                {#if entry}
                  <span class="text-xs text-muted ml-1">
                    ({entry.file_count}
                    {#if entry.total_bytes != null}
                      · {formatSize($t, entry.total_bytes)}{/if})
                  </span>
                {/if}
              </label>
            </li>
          {/each}
        </ul>
      </div>

      <div class="flex justify-between mt-6">
        <button type="button" class="btn btn-secondary" onclick={onClose}>
          {$t('common.cancel')}
        </button>
        <button
          type="button"
          class="btn btn-primary"
          disabled={selected.size === 0 ||
            !targetName.trim() ||
            importing ||
            (needsManualVersion && !mcVersionInput.trim())}
          onclick={doImport}
          data-testid="import-btn"
        >
          {$t('instances.import.importBtn')}
        </button>
      </div>
    {/if}
  </div>
</Modal>

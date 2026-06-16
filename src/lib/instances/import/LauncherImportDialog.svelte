<script lang="ts">
  import { onMount } from 'svelte';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import type { ContentCategory, ForeignInstance, LoaderKind } from '$lib/ipc/bindings';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { formatSize } from '$lib/format/size';
  import { t } from '$lib/i18n';
  import { enqueueLauncherImport } from '$lib/ops/op-queue.svelte';
  import { shouldWarnVanillaWithMods } from '$lib/instances/import/vanilla-mods-warning';
  import McVersionCombobox from '$lib/mods/McVersionCombobox.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Select from '$lib/ui/Select.svelte';
  import { Icon, type IconName } from '$lib/ui/icons';

  // Two-step wizard:
  //   Step 1 — discovery list (auto-scan + browse-to-folder)
  //   Step 2 — category checkboxes + name/version/loader for the chosen instance

  let { onClose }: { onClose: () => void } = $props();

  // ── step tracking ──────────────────────────────────────────────────────────
  let step = $state<'discover' | 'configure'>('discover');
  let chosen = $state<ForeignInstance | null>(null);

  // ── step 1: discovery ──────────────────────────────────────────────────────
  let discovering = $state(true); // auto-scan kicks off on mount
  let discovered = $state<ForeignInstance[]>([]);
  let discoverError = $state<string | null>(null);

  async function discover() {
    discovering = true;
    discoverError = null;
    try {
      const res = await commands.launcherImportDiscover();
      if (res.status === 'ok') {
        discovered = res.data;
      } else {
        discoverError = formatError(res.error);
      }
    } finally {
      discovering = false;
    }
  }

  // Scan known launcher locations as soon as the dialog opens — finding
  // importable instances is the whole point of this screen.
  onMount(() => {
    void discover();
  });

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
  // Sources whose version/loader are unknown or only best-effort: the user
  // confirms/corrects them. raw_minecraft arrives blank; the profile sources
  // (Minecraft Launcher / TLauncher) arrive pre-seeded with detection.
  const allowsVersionLoaderEdit = $derived(
    chosen?.source === 'raw_minecraft' ||
      chosen?.source === 'mojang_launcher' ||
      chosen?.source === 'tlauncher',
  );
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

  const showVanillaWithModsWarning = $derived(
    shouldWarnVanillaWithMods(loaderInput, chosen?.content ?? []),
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

  function categoryIcon(cat: ContentCategory): IconName {
    return cat === 'mods'
      ? 'puzzle'
      : cat === 'config'
        ? 'settings'
        : cat === 'saves'
          ? 'globe'
          : cat === 'resource_packs'
            ? 'resourcePack'
            : cat === 'shaderpacks'
              ? 'shader'
              : 'scrollText';
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
              : source === 'mojang_launcher'
                ? 'instances.import.sourceMojang'
                : source === 'tlauncher'
                  ? 'instances.import.sourceTlauncher'
                  : 'instances.import.sourceRaw';
    return $t(key as Parameters<typeof $t>[0]);
  }

  let importing = $state(false);

  const canImport = $derived(
    !!chosen &&
      selected.size > 0 &&
      targetName.trim() !== '' &&
      !importing &&
      (!allowsVersionLoaderEdit || mcVersionInput.trim() !== ''),
  );

  function doImport() {
    if (!chosen || !canImport) return;
    importing = true;
    enqueueLauncherImport(targetName.trim(), {
      foreign: chosen,
      selected: [...selected],
      targetName: targetName.trim(),
      mcVersionOverride: allowsVersionLoaderEdit ? mcVersionInput.trim() : null,
      loaderOverride: allowsVersionLoaderEdit ? loaderInput : null,
      loaderVersionOverride: null,
    });
    onClose();
  }
</script>

<Modal
  {onClose}
  ariaLabel={$t('instances.import.dialogAriaLabel')}
  ariaLabelledby="launcher-import-heading"
  dataTestid="launcher-import-dialog"
  panelClass="max-w-xl w-full max-h-[85vh] flex flex-col"
>
  {#if step === 'discover'}
    <!-- Step 1: discovery ─────────────────────────────────────────────── -->
    <header class="px-5 py-4 border-b border-border-subtle">
      <h2 class="text-lg font-semibold text-primary" id="launcher-import-heading">
        {$t('instances.import.step1Title')}
      </h2>
      <p class="text-sm text-muted mt-0.5">{$t('instances.import.discoverSubtitle')}</p>
    </header>

    <div class="flex-1 overflow-y-auto px-5 py-4">
      {#if discovering}
        <div class="flex flex-col items-center justify-center gap-3 py-12 text-muted">
          <Icon name="refresh" size={26} class="animate-spin" />
          <span class="text-sm">{$t('instances.import.discovering')}</span>
        </div>
      {:else if discoverError}
        <div
          class="flex items-start gap-2 rounded-md bg-danger-bg px-3 py-2 text-sm text-danger"
          role="alert"
          data-testid="discover-error"
        >
          <Icon name="warning" size={14} class="mt-0.5 shrink-0" />
          <span>{discoverError}</span>
        </div>
      {:else if discovered.length === 0}
        <div
          class="flex flex-col items-center justify-center gap-2 py-12 text-center"
          data-testid="discover-empty"
        >
          <Icon name="package" size={30} class="text-placeholder" />
          <p class="max-w-xs text-sm text-muted">{$t('instances.import.discoverEmpty')}</p>
        </div>
      {:else}
        <ul class="space-y-2" data-testid="discovered-list">
          {#each discovered as inst (inst.root)}
            <li>
              <button
                type="button"
                class="group flex w-full items-center gap-3 rounded-lg border border-border-subtle bg-surface p-3 text-left transition-colors hover:border-accent hover:bg-accent-soft"
                onclick={() => selectInstance(inst)}
                data-testid="instance-row"
              >
                <span
                  class="grid h-9 w-9 shrink-0 place-items-center rounded-md bg-subtle text-secondary transition-colors group-hover:text-accent"
                >
                  <Icon name="package" size={18} />
                </span>
                <span class="min-w-0 flex-1">
                  <span class="block truncate font-medium text-primary">{inst.name}</span>
                  <span class="block truncate text-xs text-muted">
                    {$t('instances.import.mcLabel', { version: inst.mc_version })}
                    {#if inst.loader !== 'vanilla'}
                      · {inst.loader}{inst.loader_version ? ` ${inst.loader_version}` : ''}
                    {/if}
                  </span>
                </span>
                <span
                  class="shrink-0 rounded-full bg-subtle px-2 py-0.5 text-[10px] uppercase tracking-wide text-muted"
                >
                  {sourceLabel(inst.source)}
                </span>
                <Icon
                  name="chevronRight"
                  size={16}
                  class="shrink-0 text-placeholder transition-colors group-hover:text-accent"
                />
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <footer class="flex items-center justify-between gap-2 border-t border-border-subtle px-5 py-3">
      <button
        type="button"
        class="btn-secondary btn-sm inline-flex items-center gap-1.5"
        onclick={browseFolder}
        data-testid="browse-folder-btn"
      >
        <Icon name="folderOpen" size={14} />
        {$t('instances.import.browseFolder')}
      </button>
      <div class="flex items-center gap-2">
        <button
          type="button"
          class="btn-ghost btn-sm inline-flex items-center gap-1.5"
          onclick={discover}
          disabled={discovering}
          data-testid="discover-btn"
        >
          <Icon name="refresh" size={14} class={discovering ? 'animate-spin' : ''} />
          {$t('instances.import.discover')}
        </button>
        <button type="button" class="btn-secondary btn-sm" onclick={onClose}>
          {$t('common.cancel')}
        </button>
      </div>
    </footer>
  {:else if step === 'configure' && chosen}
    <!-- Step 2: configure ─────────────────────────────────────────────── -->
    <header class="flex items-center gap-2 border-b border-border-subtle px-4 py-3">
      <button
        type="button"
        class="btn-icon"
        aria-label={$t('instances.import.back')}
        onclick={() => (step = 'discover')}
        data-testid="back-btn"
      >
        <Icon name="arrowLeft" size={18} />
      </button>
      <h2
        class="min-w-0 flex-1 truncate text-lg font-semibold text-primary"
        id="launcher-import-heading"
      >
        {$t('instances.import.step2Title', { name: chosen.name })}
      </h2>
    </header>

    <div class="flex-1 overflow-y-auto px-5 py-4 space-y-4">
      <!-- Name -->
      <label class="block">
        <span class="text-sm font-medium text-secondary">{$t('instances.import.nameLabel')}</span>
        <input
          type="text"
          class="input mt-1 w-full"
          bind:value={targetName}
          data-testid="name-input"
        />
      </label>

      <!-- Generic .minecraft: user supplies version + loader -->
      {#if allowsVersionLoaderEdit}
        <label class="block">
          <span class="text-sm font-medium text-secondary"
            >{$t('instances.import.mcVersionInputLabel')}</span
          >
          <div class="mt-1">
            <McVersionCombobox
              bind:value={mcVersionInput}
              placeholder={$t('instances.import.mcVersionPlaceholder')}
              dataTestid="mc-version-input"
            />
          </div>
          {#if chosen?.source === 'raw_minecraft'}
            <span class="mt-1 block text-xs text-muted">{$t('instances.import.mcVersionHint')}</span
            >
          {/if}
        </label>
        <div class="block">
          <span class="text-sm font-medium text-secondary"
            >{$t('instances.import.loaderInputLabel')}</span
          >
          <Select
            class="mt-1 w-full"
            value={loaderInput}
            options={LOADER_OPTIONS}
            onChange={(v) => (loaderInput = v as LoaderKind)}
            ariaLabel={$t('instances.import.loaderInputLabel')}
            dataTestid="loader-select"
          />
        </div>
        {#if showVanillaWithModsWarning}
          <p
            class="rounded-lg border border-warning-text bg-warning-bg px-3 py-2 text-sm text-warning-text"
            data-testid="vanilla-mods-warning"
          >
            {$t('instances.import.vanillaWithModsWarning')}
          </p>
        {/if}
      {/if}

      <!-- Content categories -->
      <div>
        <div class="mb-2 flex items-center justify-between">
          <span class="text-sm font-medium text-secondary"
            >{$t('instances.import.contentLabel')}</span
          >
          <button
            type="button"
            class="text-xs text-accent hover:underline"
            onclick={toggleAll}
            data-testid="toggle-all-btn"
          >
            {allSelected ? $t('instances.import.deselectAll') : $t('instances.import.selectAll')}
          </button>
        </div>

        {#if availableCategories.length === 0}
          <p class="rounded-md bg-subtle px-3 py-3 text-sm text-muted">
            {$t('instances.import.noContent')}
          </p>
        {:else}
          <ul class="space-y-0.5" data-testid="category-list">
            {#each availableCategories as cat (cat)}
              {@const entry = contentEntry(cat)}
              <li>
                <label
                  for={`cat-${cat}`}
                  class="flex cursor-pointer items-center gap-3 rounded-md px-2 py-2 transition-colors hover:bg-subtle"
                >
                  <input
                    type="checkbox"
                    id={`cat-${cat}`}
                    checked={selected.has(cat)}
                    onchange={() => toggleCategory(cat)}
                    class="rounded"
                    data-testid={`cat-${cat}`}
                  />
                  <Icon name={categoryIcon(cat)} size={16} class="shrink-0 text-secondary" />
                  <span class="flex-1 text-sm text-primary">{categoryLabel(cat)}</span>
                  {#if entry}
                    <span class="shrink-0 text-xs text-muted">
                      {entry.file_count}{#if entry.total_bytes != null}
                        · {formatSize($t, entry.total_bytes)}{/if}
                    </span>
                  {/if}
                </label>
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <!-- Source folder (so the user can find / clean up the original) -->
      <div class="flex items-start gap-2 rounded-md bg-subtle px-3 py-2 text-xs">
        <Icon name="folderOpen" size={14} class="mt-0.5 shrink-0 text-muted" />
        <div class="min-w-0">
          <div class="text-secondary">{$t('instances.import.sourcePathLabel')}</div>
          <div class="break-all font-mono text-muted" data-testid="source-path">{chosen.root}</div>
        </div>
      </div>
    </div>

    <footer class="flex justify-between gap-2 border-t border-border-subtle px-5 py-3">
      <button type="button" class="btn-secondary btn-sm" onclick={onClose}>
        {$t('common.cancel')}
      </button>
      <button
        type="button"
        class="btn-primary btn-sm"
        disabled={!canImport}
        onclick={doImport}
        data-testid="import-btn"
      >
        {$t('instances.import.importBtn')}
      </button>
    </footer>
  {/if}
</Modal>

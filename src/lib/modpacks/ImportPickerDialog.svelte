<script lang="ts">
  import type { ModpackFile, ModpackSummary, ModpackUnresolvable } from '$lib/ipc/bindings';
  import { formatSize } from '$lib/format/size';
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import { Icon } from '$lib/ui/icons';
  import type { TranslationKey } from '$lib/i18n/keys.generated';

  // Modal that shows the parsed `ModpackSummary` to the user and lets them
  // choose which optional mods to install. Required mods are listed but
  // not toggleable (the pack author marked them mandatory). Unresolvable
  // entries — files the backend cannot auto-download due to
  // distribution-disabled or host-allowlist constraints — render as a
  // red list with manual-action links the user clicks to fetch the mod
  // themselves (the installed-mods view will pick them up on next launch).
  //
  // No IPC here. The owning view drives `modpack_import` when `onConfirm`
  // fires with the chosen sha1 list (required + selected optionals).

  let {
    summary,
    onCancel,
    onConfirm,
  }: {
    summary: ModpackSummary;
    onCancel: () => void;
    onConfirm: (selectedShas: string[]) => void;
  } = $props();

  // Selection logic: required/optional split (env_client-based).
  const required: ModpackFile[] = $derived(
    summary.files.filter((f) => f.env_client === 'required'),
  );
  const optional: ModpackFile[] = $derived(
    summary.files.filter((f) => f.env_client === 'optional'),
  );
  const unresolvable: ModpackUnresolvable[] = $derived(summary.unresolvable);

  // Optional selections — Set keyed by sha1. Defaults to empty so optional
  // mods are off by default; the user opts in explicitly.
  let optionalSelected = $state<Set<string>>(new Set());

  function toggle(sha: string) {
    // Re-assign so Svelte 5's $state reactivity picks up the change
    // (Set mutations in place do not trigger reactivity).
    const next = new Set(optionalSelected);
    if (next.has(sha)) next.delete(sha);
    else next.add(sha);
    optionalSelected = next;
  }

  // Deduplicated sha1 list for the install payload. FTB packs can have
  // duplicate sha1s across files; dedupe so the "Install N" count and
  // the payload the backend receives are clean.
  const selectedShas = $derived([
    ...new Set([
      ...required.map((f) => f.sha1),
      ...optional.filter((f) => optionalSelected.has(f.sha1)).map((f) => f.sha1),
    ]),
  ]);

  // ── Category grouping ───────────────────────────────────────────────────
  type CategoryKey =
    | 'groupMods'
    | 'groupResourcepacks'
    | 'groupShaderpacks'
    | 'groupConfig'
    | 'groupScripts'
    | 'groupOther';

  function categorise(installPath: string): CategoryKey {
    const p = installPath.toLowerCase();
    if (p.startsWith('mods/')) return 'groupMods';
    if (p.startsWith('resourcepacks/')) return 'groupResourcepacks';
    if (p.startsWith('shaderpacks/')) return 'groupShaderpacks';
    if (p.startsWith('config/') || p.startsWith('defaultconfigs/')) return 'groupConfig';
    if (p.startsWith('kubejs/') || p.startsWith('scripts/')) return 'groupScripts';
    return 'groupOther';
  }

  const CATEGORY_ORDER: CategoryKey[] = [
    'groupMods',
    'groupResourcepacks',
    'groupShaderpacks',
    'groupConfig',
    'groupScripts',
    'groupOther',
  ];

  // Static map from CategoryKey to its i18n key — greppable and compile-time
  // checked via TranslationKey (no dynamic key construction at runtime).
  const GROUP_LABEL_KEY: Record<CategoryKey, TranslationKey> = {
    groupMods: 'modpacks.import.picker.groupMods',
    groupResourcepacks: 'modpacks.import.picker.groupResourcepacks',
    groupShaderpacks: 'modpacks.import.picker.groupShaderpacks',
    groupConfig: 'modpacks.import.picker.groupConfig',
    groupScripts: 'modpacks.import.picker.groupScripts',
    groupOther: 'modpacks.import.picker.groupOther',
  };

  interface FileGroup {
    key: CategoryKey;
    files: ModpackFile[];
    totalSize: number;
  }

  const fileGroups = $derived.by((): FileGroup[] => {
    const map = new Map<CategoryKey, ModpackFile[]>();
    for (const key of CATEGORY_ORDER) map.set(key, []);
    for (const f of summary.files) {
      map.get(categorise(f.install_path))!.push(f);
    }
    const result: FileGroup[] = [];
    for (const key of CATEGORY_ORDER) {
      const files = map.get(key)!;
      if (files.length === 0) continue;
      const totalSize = files.reduce((acc, f) => acc + (f.size ?? 0), 0);
      result.push({ key, files, totalSize });
    }
    return result;
  });
</script>

<Modal
  ariaLabelledby="import-picker-title"
  onClose={onCancel}
  panelClass="max-w-2xl w-full max-h-[80vh] flex flex-col"
>
  <header class="p-4 border-b">
    <h2 id="import-picker-title" class="text-lg font-semibold text-primary">{summary.name}</h2>
    <div class="text-sm text-muted">
      v{summary.version} ·
      {summary.format === 'modrinth'
        ? 'Modrinth .mrpack'
        : summary.format === 'ftb'
          ? 'FTB'
          : summary.format === 'atlauncher'
            ? 'ATLauncher'
            : 'CurseForge .zip'}
      · MC {summary.game_version} · {summary.loader}{summary.loader_version
        ? ` ${summary.loader_version}`
        : ''}
    </div>
  </header>

  {#if summary.has_saves_in_overrides}
    <div
      class="m-4 p-3 bg-warning-bg border border-warning-text/30 rounded text-sm text-warning-text flex items-start gap-1.5"
    >
      <Icon name="warning" size={14} class="flex-shrink-0 mt-0.5" />
      <span>{$t('modpacks.import.picker.savesWarning')}</span>
    </div>
  {/if}

  <div class="flex-1 overflow-y-auto p-4 space-y-2">
    {#each fileGroups as group (group.key)}
      {@const sizeStr = formatSize($t, group.totalSize)}
      <details open={group.key === 'groupMods'}>
        <summary
          class="font-medium text-sm text-primary cursor-pointer select-none list-none flex items-center gap-1 py-1"
        >
          <span class="disclosure-caret mr-1"><Icon name="caret" size={12} /></span>
          {#if sizeStr}
            {$t('modpacks.import.picker.groupHeader', {
              label: $t(GROUP_LABEL_KEY[group.key]),
              count: group.files.length,
              size: sizeStr,
            })}
          {:else}
            {$t('modpacks.import.picker.groupHeaderNoSize', {
              label: $t(GROUP_LABEL_KEY[group.key]),
              count: group.files.length,
            })}
          {/if}
        </summary>
        <ul class="space-y-1 mt-1 mb-2 pl-4">
          {#each group.files as f (f.install_path)}
            {@const isRequired = f.env_client === 'required'}
            <li class="text-sm py-1 flex items-center">
              {#if isRequired}
                <input
                  type="checkbox"
                  checked
                  disabled
                  class="mr-2"
                  aria-label={$t('modpacks.import.picker.requiredModAriaLabel', {
                    name: f.name,
                  })}
                />
              {:else}
                <input
                  type="checkbox"
                  checked={optionalSelected.has(f.sha1)}
                  onchange={() => toggle(f.sha1)}
                  class="mr-2"
                  aria-label={$t('modpacks.import.picker.installModAriaLabel', { name: f.name })}
                />
              {/if}
              <span>{f.name}</span>
              <span class="ml-auto text-placeholder text-xs">{formatSize($t, f.size)}</span>
            </li>
          {/each}
        </ul>
      </details>
    {/each}

    {#if unresolvable.length > 0}
      <details>
        <summary
          class="font-medium text-sm text-primary cursor-pointer select-none list-none flex items-center gap-1 py-1"
        >
          <span class="disclosure-caret mr-1"><Icon name="caret" size={12} /></span>
          {$t('modpacks.import.picker.cannotAutoInstall', { count: unresolvable.length })}
        </summary>
        <ul class="space-y-1 mt-1 mb-2 pl-4">
          {#each unresolvable as u, i (i)}
            <li class="text-sm py-1 flex items-center bg-danger-bg px-2 rounded">
              <span class="flex-1">{u.mod_name}</span>
              {#if u.manual_action_url}
                <button
                  type="button"
                  onclick={() =>
                    void import('@tauri-apps/plugin-opener').then((m) =>
                      m.openUrl(u.manual_action_url),
                    )}
                  class="text-accent hover:underline text-xs inline-flex items-center gap-1"
                  >{$t('modpacks.import.picker.openLink')}<Icon
                    name="externalLink"
                    size={12}
                  /></button
                >
              {/if}
            </li>
          {/each}
        </ul>
      </details>
    {/if}
  </div>

  <footer class="p-4 border-t flex justify-end gap-2">
    <button type="button" class="btn-secondary btn-sm" onclick={onCancel}
      >{$t('common.cancel')}</button
    >
    <button
      type="button"
      class="btn-primary btn-sm"
      disabled={selectedShas.length === 0}
      onclick={() => onConfirm(selectedShas)}
    >
      {$t('modpacks.import.picker.installBtn', { count: selectedShas.length })}
    </button>
  </footer>
</Modal>

<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { defaultCloneName, INSTANCE_NAME_MAX } from '$lib/instances/clone-request';
  import { categoryLabelKey } from '$lib/instances/import/category-display';
  import type { CloneOptions, ContentEntry, InstanceWithStatus } from '$lib/ipc/bindings';
  import { commands } from '$lib/ipc/bindings';
  import { formatSize } from '$lib/format/size';
  import { t } from '$lib/i18n';
  import { enqueueClone } from '$lib/ops/op-queue.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { Icon, type IconName } from '$lib/ui/icons';

  // Clone dialog: name + six what-to-copy checkboxes (all on by default).
  // Mods + installed-mods registry + icon always travel with the clone —
  // stated as a static note, not a checkbox. Confirm enqueues the op and
  // closes immediately; the op-queue owns progress + the completion toast.

  let { instance, onClose }: { instance: InstanceWithStatus; onClose: () => void } = $props();

  // Seeded once — the source instance can't change while the dialog is open.
  let name = $state('');
  let options = $state<CloneOptions>({
    saves: true,
    settings: true,
    packs: true,
    config: true,
    options_txt: true,
    playtime: true,
  });

  // Per-category file counts + sizes for the checkbox labels (best-effort:
  // a failed scan just leaves the labels bare and every box enabled).
  let content = $state<ContentEntry[] | null>(null);
  // null until getPlaytime resolves; then whether any session was recorded.
  let hasPlaytime = $state<boolean | null>(null);

  onMount(() => {
    name = defaultCloneName(instance.name, get(t)('instance.clone.defaultSuffix'));
    void (async () => {
      const scan = await commands.cloneInstanceScan(instance.id);
      if (scan.status === 'ok') {
        content = scan.data;
        // Uncheck what there is nothing to copy of, so the enqueue reflects
        // reality (the backend tolerates either way).
        for (const key of ['saves', 'config', 'options_txt', 'packs'] as const) {
          if (contentDisabled(key)) options[key] = false;
        }
      }
      const stats = await commands.getPlaytime(instance.id);
      if (stats.status === 'ok') {
        hasPlaytime = stats.data.session_count > 0;
        if (!hasPlaytime) options.playtime = false;
      }
    })();
  });

  function entryFor(category: ContentEntry['category']): ContentEntry | null {
    return content?.find((c) => c.category === category) ?? null;
  }

  /** Files + bytes label for one option row; packs sums both categories. */
  function sizeLabel(key: 'saves' | 'config' | 'options_txt' | 'packs' | 'mods'): string {
    if (content === null) return '';
    const entries = (
      key === 'packs' ? [entryFor('resource_packs'), entryFor('shaderpacks')] : [entryFor(key)]
    ).filter((e): e is ContentEntry => e !== null);
    if (entries.length === 0) return '';
    const files = entries.reduce((n, e) => n + e.file_count, 0);
    const bytes = entries.reduce((n, e) => n + (e.total_bytes ?? 0), 0);
    return `${files} · ${formatSize(get(t), bytes)}`;
  }

  /** True once the scan proves there is nothing to copy for this option. */
  function contentDisabled(key: 'saves' | 'config' | 'options_txt' | 'packs'): boolean {
    if (content === null) return false;
    if (key === 'packs') {
      return entryFor('resource_packs') === null && entryFor('shaderpacks') === null;
    }
    return entryFor(key) === null;
  }

  type OptionKey = keyof CloneOptions;
  type Row = { key: OptionKey; label: string; icon: IconName; disabled: boolean; hint: string };

  const rows = $derived<Row[]>([
    {
      key: 'saves',
      label: $t(categoryLabelKey('saves')),
      icon: 'globe',
      disabled: contentDisabled('saves'),
      hint: sizeLabel('saves'),
    },
    {
      key: 'settings',
      label: $t('instance.clone.optionSettings'),
      icon: 'sliders',
      disabled: false,
      hint: '',
    },
    {
      key: 'packs',
      label: $t('instance.clone.optionPacks'),
      icon: 'resourcePack',
      disabled: contentDisabled('packs'),
      hint: sizeLabel('packs'),
    },
    {
      key: 'config',
      label: $t(categoryLabelKey('config')),
      icon: 'settings',
      disabled: contentDisabled('config'),
      hint: sizeLabel('config'),
    },
    {
      key: 'options_txt',
      label: $t(categoryLabelKey('options_txt')),
      icon: 'scrollText',
      disabled: contentDisabled('options_txt'),
      hint: sizeLabel('options_txt'),
    },
    {
      key: 'playtime',
      label: $t('instance.clone.optionPlaytime'),
      icon: 'play',
      disabled: hasPlaytime === false,
      hint: hasPlaytime === false ? $t('instance.clone.noPlaytime') : '',
    },
  ]);

  const canSubmit = $derived(name.trim() !== '');

  function submit() {
    const trimmed = name.trim();
    if (!trimmed) return;
    enqueueClone(trimmed, {
      sourceId: instance.id,
      newName: trimmed,
      options: { ...options },
    });
    onClose();
  }
</script>

<Modal
  {onClose}
  ariaLabelledby="clone-instance-heading"
  dataTestid="clone-instance-dialog"
  panelClass="max-w-md w-full max-h-[85vh] flex flex-col"
>
  <header class="flex items-center justify-between border-b border-border-subtle px-5 py-3">
    <h2 class="min-w-0 truncate text-lg font-semibold text-primary" id="clone-instance-heading">
      {$t('instance.clone.title', { name: instance.name })}
    </h2>
    <CloseButton onClick={onClose} ariaLabel={$t('common.cancel')} />
  </header>

  <div class="flex-1 space-y-4 overflow-y-auto px-5 py-4">
    <label class="block">
      <span class="mb-1 flex justify-between text-sm font-medium text-secondary">
        <span>{$t('instance.clone.nameLabel')}</span>
        <span class="font-normal text-placeholder">
          {$t('instance.manage.nameCounter', { count: name.length, max: INSTANCE_NAME_MAX })}
        </span>
      </span>
      <input
        type="text"
        class="input mt-1 w-full"
        maxlength={INSTANCE_NAME_MAX}
        bind:value={name}
        data-testid="clone-name-input"
      />
    </label>

    <div>
      <span class="mb-2 block text-sm font-medium text-secondary">
        {$t('instance.clone.whatToCopy')}
      </span>
      <ul class="space-y-0.5" data-testid="clone-option-list">
        {#each rows as row (row.key)}
          <li>
            <label
              for={`clone-opt-${row.key}`}
              class="flex items-center gap-3 rounded-md px-2 py-2 transition-colors"
              class:cursor-pointer={!row.disabled}
              class:hover:bg-subtle={!row.disabled}
              class:opacity-50={row.disabled}
            >
              <input
                type="checkbox"
                id={`clone-opt-${row.key}`}
                class="rounded"
                checked={options[row.key]}
                disabled={row.disabled}
                onchange={() => (options[row.key] = !options[row.key])}
                data-testid={`clone-opt-${row.key}`}
              />
              <Icon name={row.icon} size={16} class="shrink-0 text-secondary" />
              <span class="flex-1 text-sm text-primary">{row.label}</span>
              {#if row.hint}
                <span class="shrink-0 text-xs text-muted">{row.hint}</span>
              {/if}
            </label>
          </li>
        {/each}
      </ul>
      <p
        class="mt-2 flex items-start gap-2 rounded-md bg-subtle px-3 py-2 text-xs text-muted"
        data-testid="clone-mods-always"
      >
        <Icon name="puzzle" size={14} class="mt-0.5 shrink-0" />
        <span>
          {$t('instance.clone.modsAlways')}
          {#if sizeLabel('mods')}
            · {sizeLabel('mods')}
          {/if}
        </span>
      </p>
    </div>
  </div>

  <footer class="flex items-center justify-end gap-2 border-t border-border-subtle px-5 py-3">
    <button type="button" class="btn-secondary btn-sm" onclick={onClose}>
      {$t('common.cancel')}
    </button>
    <button
      type="button"
      class="btn-primary btn-sm inline-flex items-center gap-1.5"
      disabled={!canSubmit}
      onclick={submit}
      data-testid="clone-submit"
    >
      <Icon name="copy" size={14} />
      {$t('instance.clone.cloneBtn')}
    </button>
  </footer>
</Modal>

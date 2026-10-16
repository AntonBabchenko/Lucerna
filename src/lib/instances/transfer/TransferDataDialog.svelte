<script lang="ts">
  import { get } from 'svelte/store';
  import {
    commands,
    events,
    type InstanceWithStatus,
    type OnConflict,
    type Refusal,
    type RootPreview,
    type TransferPreview,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { listenUntilDestroyed } from '$lib/ipc/listen';
  import { formatSize } from '$lib/format/size';
  import { t } from '$lib/i18n';
  import { displayLoader } from '$lib/instances/loader-display';
  import ContentLocationNote from '$lib/instances/import/ContentLocationNote.svelte';
  import { enqueueTransfer } from '$lib/ops/op-queue.svelte';
  import { taskFor } from '$lib/tasks/registry.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { Icon, type IconName } from '$lib/ui/icons';
  import {
    ITEM_ICON,
    LOD_MULTIPLAYER,
    MULTIPLAYER_ONLY,
    VERSION_SENSITIVE,
    itemLabelKey,
    refusalText,
    selectionFrom,
  } from './transfer-view';

  // The mod-data transfer (spec 2026-10-15 §A1): pick another instance, see
  // what of its mod data it has (and what of it is already here), choose what
  // to copy and what to do with same-named files. Copy enqueues the op and
  // closes; the op queue owns progress and the completion toast. The source is
  // only read — the backend holds its claim while copying.

  let {
    target,
    instances,
    onClose,
  }: {
    target: InstanceWithStatus;
    instances: InstanceWithStatus[];
    onClose: () => void;
  } = $props();

  // Running state as MigrateWorldDialog reads it: one fetch, then the events.
  let runningIds = $state<string[]>([]);
  void (async () => {
    try {
      runningIds = (await commands.runningInstances()).map((r) => r.instance_id);
    } catch {
      // Could not tell: the options stay enabled and the backend's claim
      // refuses a running source with a named error.
    }
  })();
  listenUntilDestroyed([
    events.processSpawned.listen((e) => {
      const id = e.payload.instance_id;
      if (!runningIds.includes(id)) runningIds = [...runningIds, id];
    }),
    events.processExited.listen((e) => {
      runningIds = runningIds.filter((id) => id !== e.payload.instance_id);
    }),
  ]);

  const sources = $derived(instances.filter((i) => i.id !== target.id));

  function sourceState(i: InstanceWithStatus): 'running' | 'busy' | null {
    if (runningIds.includes(i.id)) return 'running';
    if (taskFor({ instanceId: i.id }) !== null) return 'busy';
    return null;
  }

  const sourceOptions = $derived(
    sources.map((i) => {
      const state = sourceState(i);
      const base = `${i.name} · ${displayLoader(i.loader)} ${i.mc_version}`;
      const suffix =
        state === 'running'
          ? $t('instance.transfer.sourceRunning')
          : state === 'busy'
            ? $t('instance.transfer.sourceBusy')
            : null;
      return {
        value: i.id,
        label: suffix ? `${base} (${suffix})` : base,
        disabled: state !== null,
      };
    }),
  );

  let sourceId = $state<string | null>(null);
  const source = $derived(sources.find((i) => i.id === sourceId) ?? null);
  let preview = $state<TransferPreview | null>(null);
  let scanError = $state<string | null>(null);
  let scanning = $state(false);
  let items = $state<Record<string, boolean>>({});
  let other = $state<Record<string, boolean>>({});
  let onConflict = $state<OnConflict>('replace');

  async function pick(id: string) {
    sourceId = id;
    preview = null;
    scanError = null;
    scanning = true;
    const r = await commands.transferScan(id, target.id);
    if (sourceId !== id) return; // another source was picked meanwhile
    scanning = false;
    if (r.status === 'error') {
      scanError = formatError(r.error);
      return;
    }
    preview = r.data;
    // Catalog rows start checked (what the user came for); "Other folders"
    // start unchecked (some are caches). A refused row can never be checked.
    items = Object.fromEntries(r.data.items.map((i) => [i.id, i.refusal === null]));
    other = Object.fromEntries(r.data.other.map((o) => [o.name, false]));
  }

  type Counted = { bytes: number; conflicts: number };
  const checkedRows = $derived<Counted[]>(
    preview === null
      ? []
      : [
          ...preview.items.filter((i) => items[i.id] && i.refusal === null),
          ...preview.other.filter((o) => other[o.name] && o.refusal === null),
        ],
  );
  const anyConflict = $derived(checkedRows.some((r) => r.conflicts > 0));
  const needBytes = $derived(checkedRows.reduce((n, r) => n + r.bytes, 0));
  const freeBytes = $derived(
    preview?.target_free_mb == null ? null : preview.target_free_mb * 1024 * 1024,
  );
  const spaceShort = $derived(freeBytes !== null && needBytes > freeBytes);
  const canSubmit = $derived(source !== null && checkedRows.length > 0);

  function sizeHint(files: number, bytes: number): string {
    return $t('instance.transfer.filesSize', { files, size: formatSize(get(t), bytes) });
  }

  function submit() {
    if (!source || !canSubmit) return;
    enqueueTransfer({
      sourceId: source.id,
      sourceName: source.name,
      targetId: target.id,
      targetName: target.name,
      selection: selectionFrom(
        Object.fromEntries(Object.entries(items).filter(([id]) => !refusedItem(id))),
        Object.fromEntries(Object.entries(other).filter(([name]) => !refusedOther(name))),
      ),
      onConflict: anyConflict ? onConflict : 'replace',
    });
    onClose();
  }

  function refusedItem(id: string): boolean {
    return preview?.items.find((i) => i.id === id)?.refusal != null;
  }
  function refusedOther(name: string): boolean {
    return preview?.other.find((o) => o.name === name)?.refusal != null;
  }
</script>

{#snippet row(
  testid: string,
  icon: IconName,
  label: string,
  checked: boolean,
  toggle: () => void,
  r: {
    roots: RootPreview[];
    files: number;
    bytes: number;
    conflicts: number;
    refusal: Refusal | null;
  },
  notes: string[],
)}
  {@const refused = r.refusal !== null}
  <li>
    <label
      for={testid}
      class="flex items-start gap-3 rounded-md px-2 py-2 transition-colors"
      class:cursor-pointer={!refused}
      class:hover:bg-subtle={!refused}
    >
      <input
        type="checkbox"
        id={testid}
        class="mt-0.5 rounded"
        checked={checked && !refused}
        disabled={refused}
        onchange={toggle}
        data-testid={testid}
      />
      <!-- A refused row dims its name, not the line saying why. -->
      <Icon
        name={icon}
        size={16}
        class={`mt-0.5 shrink-0 text-secondary ${refused ? 'opacity-50' : ''}`}
      />
      <span class="min-w-0 flex-1">
        <span class="block text-sm text-primary" class:opacity-50={refused}>{label}</span>
        {#each r.roots as root (root.rel)}
          <ContentLocationNote location={root.source} testid={`${testid}-source-${root.rel}`} />
          {#if root.target?.kind === 'linked'}
            <span class="flex items-start gap-1 text-xs text-muted">
              <Icon name="link" size={12} class="mt-0.5 shrink-0" />
              <span class="min-w-0 break-all"
                >{$t('instance.transfer.linkedTarget', { target: root.target.target })}</span
              >
            </span>
          {/if}
        {/each}
        {#if r.refusal !== null}
          <span class="block text-xs text-warning-text" data-testid={`${testid}-refusal`}
            >{refusalText($t, r.refusal)}</span
          >
        {:else}
          {#if r.conflicts > 0}
            <span class="block text-xs text-secondary"
              >{$t('instance.transfer.alreadyHere', { count: r.conflicts })}</span
            >
          {/if}
          {#each notes as note (note)}
            <span class="block text-xs text-muted">{note}</span>
          {/each}
        {/if}
      </span>
      {#if !refused && r.files > 0}
        <span class="shrink-0 text-xs text-muted">{sizeHint(r.files, r.bytes)}</span>
      {/if}
    </label>
  </li>
{/snippet}

<Modal
  {onClose}
  ariaLabelledby="transfer-data-heading"
  dataTestid="transfer-data-dialog"
  panelClass="max-w-lg w-full max-h-[85vh] flex flex-col"
>
  <header class="flex items-center justify-between border-b border-border-subtle px-5 py-3">
    <h2 class="min-w-0 truncate text-lg font-semibold text-primary" id="transfer-data-heading">
      {$t('instance.transfer.dialogTitle', { name: target.name })}
    </h2>
    <CloseButton onClick={onClose} ariaLabel={$t('common.cancel')} />
  </header>

  <div class="flex-1 space-y-4 overflow-y-auto px-5 py-4">
    <div>
      <span class="mb-1 block text-sm font-medium text-secondary"
        >{$t('instance.transfer.sourceLegend')}</span
      >
      {#if sources.length === 0}
        <p class="text-sm text-muted" data-testid="transfer-no-sources">
          {$t('instance.transfer.noOtherInstances')}
        </p>
      {:else}
        <Select
          class="w-full"
          value={sourceId}
          options={sourceOptions}
          onChange={(v) => void pick(String(v))}
          placeholder={$t('instance.transfer.sourceLegend')}
          ariaLabel={$t('instance.transfer.sourceLegend')}
          dataTestid="transfer-source"
        />
      {/if}
    </div>

    {#if scanning && source}
      <p class="flex items-center gap-2 text-sm text-muted" data-testid="transfer-scanning">
        <Spinner size="sm" />
        {$t('instance.transfer.scanning', { name: source.name })}
      </p>
    {:else if scanError}
      <StatusMessage tone="danger" message={scanError} />
    {:else if preview && source}
      {#if preview.items.length === 0 && preview.other.length === 0}
        <p class="text-sm text-muted" data-testid="transfer-nothing">
          {$t('instance.transfer.nothingToCopy', { name: source.name })}
        </p>
      {:else}
        {#if preview.items.length > 0}
          <div>
            <span class="mb-2 block text-sm font-medium text-secondary"
              >{$t('instance.transfer.whatLegend')}</span
            >
            <ul class="space-y-0.5" data-testid="transfer-items">
              {#each preview.items as item (item.id)}
                {@const notes = [
                  ...(VERSION_SENSITIVE.has(item.id)
                    ? [$t('instance.transfer.versionCaution')]
                    : []),
                  ...(MULTIPLAYER_ONLY.has(item.id) ? [$t('instance.transfer.spMapsHint')] : []),
                  ...(LOD_MULTIPLAYER.has(item.id) ? [$t('instance.transfer.lodHint')] : []),
                ]}
                {@render row(
                  `transfer-item-${item.id}`,
                  ITEM_ICON[item.id],
                  $t(itemLabelKey(item.id)),
                  items[item.id] ?? false,
                  () => (items[item.id] = !items[item.id]),
                  item,
                  notes,
                )}
              {/each}
            </ul>
          </div>
        {/if}
        {#if preview.other.length > 0}
          <div>
            <span class="block text-sm font-medium text-secondary"
              >{$t('instance.transfer.otherLegend')}</span
            >
            <p class="mb-2 text-xs text-muted">
              {$t('instance.transfer.otherHint', { name: source.name })}
            </p>
            <ul class="space-y-0.5" data-testid="transfer-other">
              {#each preview.other as folder (folder.name)}
                {@render row(
                  `transfer-other-${folder.name}`,
                  'folderOpen',
                  folder.name,
                  other[folder.name] ?? false,
                  () => (other[folder.name] = !other[folder.name]),
                  { ...folder, roots: [folder.root] },
                  [],
                )}
              {/each}
            </ul>
          </div>
        {/if}
        {#if anyConflict}
          <fieldset class="flex flex-col gap-2" data-testid="transfer-conflict">
            <legend class="mb-1 text-sm font-medium text-secondary">
              {$t('instance.transfer.conflictLegend')}
            </legend>
            <label class="flex items-start gap-2 text-sm">
              <input
                type="radio"
                name="transfer-conflict"
                value="replace"
                bind:group={onConflict}
                data-testid="transfer-conflict-replace"
              />
              <span>{$t('instance.transfer.conflictReplace')}</span>
            </label>
            <label class="flex items-start gap-2 text-sm">
              <input
                type="radio"
                name="transfer-conflict"
                value="keep_existing"
                bind:group={onConflict}
                data-testid="transfer-conflict-keep"
              />
              <span>{$t('instance.transfer.conflictKeep')}</span>
            </label>
          </fieldset>
        {/if}
        {#if spaceShort && freeBytes !== null}
          <StatusMessage
            tone="warning"
            dataTestid="transfer-space-warning"
            message={$t('instance.transfer.spaceWarning', {
              need: formatSize(get(t), needBytes),
              free: formatSize(get(t), freeBytes),
            })}
          />
        {/if}
      {/if}
    {/if}
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
      data-testid="transfer-submit"
    >
      <Icon name="copy" size={14} />
      {$t('instance.transfer.copyBtn')}
    </button>
  </footer>
</Modal>

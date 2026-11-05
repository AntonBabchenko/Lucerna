<script lang="ts">
  // Finish installing a pack (spec 2026-11-05 §5.3): the files Lucerna could not download, a way
  // to open each, and three ways to hand them over — drop, pick, or a look in the downloads
  // folder (once, or watched while this dialog is open, when the user turns that on). Takes OS
  // file drops while it is the topmost dialog (`takesFileDrops`, DESIGN §8/§14).
  //
  // Focus (DESIGN §8): no control here is `disabled` while it may hold focus. The drop box, "Look
  // in the folder", "Open all" and "Copy links" stay focusable and say they are unavailable with
  // `aria-disabled`; their handlers ignore the click, so a button that stops applying while it is
  // focused (the last file landed, a supply is running) never strands the keyboard.
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import type { ReportLine } from '$lib/format/reason-lines';
  import { t } from '$lib/i18n';
  import {
    commands,
    events,
    type MissingModStatus,
    type ModpackStatus,
    type SupplyRow,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import FileDropzone from '$lib/mods/FileDropzone.svelte';
  import { announceRemoval } from '$lib/mods/mod-ops.svelte';
  import { generalDisplayed, patchGeneral } from '$lib/settings/app-settings.svelte';
  import { droppedManualDownloads } from '$lib/settings/state.svelte';
  import { pushInfo, pushWarning } from '$lib/toasts/toasts.svelte';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import DialogTitle from '$lib/ui/DialogTitle.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { openExternalHttps } from '$lib/ui/safe-open';
  import Spinner from '$lib/ui/Spinner.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { onWindowFocus } from '$lib/ui/window-focus';
  import { entryKey, entryLink, needsAttention } from './entry-view';
  import ManualDownloadRow from './ManualDownloadRow.svelte';
  import { noteManualDownloadsChanged } from './manual-downloads.svelte';
  import { scanResult, supplyResult } from './supply-lines';

  let { instanceId, onClose }: { instanceId: string; onClose: () => void } = $props();

  const OPEN_ALL_ASK_ABOVE = 8;
  const OPEN_ALL_GAP_MS = 300;
  const WATCH_EVERY_MS = 2000;

  type Result = {
    message: string | null;
    details: ReportLine[];
    tone: 'success' | 'warning' | 'info' | 'danger';
  };

  let status = $state<ModpackStatus | null>(null);
  let loadError = $state<string | null>(null);
  let defaultFolder = $state<string | null>(null);
  let result = $state<Result>({ message: null, details: [], tone: 'info' });
  let busy = $state(false);
  let scanning = false;
  let watchStopped = $state<string | null>(null);
  /** key → the dropped file waiting for Replace and what holds its place. */
  let found = $state<Record<string, { path: string; existing: string; mod: string }>>({});
  let ask = $state<
    | null
    | { kind: 'openAll'; urls: string[] }
    | { kind: 'unverified'; key: string; path: string; mod: string }
    | { kind: 'replace'; key: string; path: string; existing: string; mod: string }
  >(null);

  const rows = $derived(status?.missing_mods ?? []);
  const pending = $derived(rows.filter(needsAttention));
  const doneRows = $derived(rows.filter((m) => !needsAttention(m)));
  const requiredLeft = $derived(pending.filter((m) => m.entry.meta.required).length);
  const folder = $derived(generalDisplayed()?.manual_downloads_folder ?? defaultFolder);
  const watchOn = $derived(generalDisplayed()?.manual_downloads_watch ?? false);
  const links = $derived(
    pending.flatMap((m) => {
      const l = entryLink(m.entry);
      return l ? [l.url] : [];
    }),
  );
  const noLinks = $derived(links.length === 0);
  const cannotScan = $derived(folder === null || busy);

  const failed = (error: Parameters<typeof formatError>[0]): Result => ({
    message: formatError(error),
    details: [],
    tone: 'danger',
  });

  async function load(): Promise<void> {
    const r = await commands.modpackStatus(instanceId);
    if (r.status === 'ok') {
      status = r.data;
      loadError = null;
    } else {
      loadError = formatError(r.error);
    }
  }

  function remember(rowsIn: readonly SupplyRow[]): void {
    const next = { ...found };
    for (const r of rowsIn) {
      if (r.kind === 'needs_replace') {
        next[r.key] = { path: r.path, existing: r.existing, mod: r.mod_name };
      }
      if (r.kind === 'added' || r.kind === 'already_there') delete next[r.key];
    }
    found = next;
  }

  async function supply(paths: string[]): Promise<void> {
    if (paths.length === 0) return;
    busy = true;
    try {
      const r = await commands.manualDownloadsSupply(instanceId, paths);
      if (r.status === 'ok') {
        remember(r.data.rows);
        result = supplyResult(get(t), r.data.rows);
        noteManualDownloadsChanged();
      } else {
        result = failed(r.error);
      }
    } finally {
      busy = false;
      await load();
    }
  }

  async function scan(quiet: boolean): Promise<void> {
    if (scanning) return;
    scanning = true;
    try {
      const r = await commands.manualDownloadsScan(instanceId, folder);
      if (r.status === 'ok') {
        remember(r.data.rows);
        if (r.data.rows.length > 0) noteManualDownloadsChanged();
        await load();
        if (!quiet || r.data.rows.length > 0 || r.data.skipped_unreadable > 0) {
          result = scanResult(get(t), r.data, pending.length);
        }
      } else if (quiet) {
        // A failed poll stops the watch for this dialog and says why; the saved choice stays on.
        watchStopped = formatError(r.error);
      } else {
        result = failed(r.error);
      }
    } finally {
      scanning = false;
    }
  }

  function pickerFilters() {
    return [{ name: get(t)('modpacks.manual.fileFilter'), extensions: ['jar', 'zip'] }];
  }

  async function chooseFiles(): Promise<void> {
    if (busy) return;
    const r = await openFile({
      multiple: true,
      defaultPath: folder ?? undefined,
      filters: pickerFilters(),
    });
    if (Array.isArray(r)) await supply(r);
  }

  async function changeFolder(): Promise<void> {
    const r = await openFile({ directory: true, defaultPath: folder ?? undefined });
    if (typeof r === 'string') {
      watchStopped = null;
      await patchGeneral({ manual_downloads_folder: r });
    }
  }

  async function useFile(m: MissingModStatus): Promise<void> {
    const r = await openFile({
      multiple: false,
      defaultPath: folder ?? undefined,
      filters: pickerFilters(),
    });
    if (typeof r === 'string') {
      ask = { kind: 'unverified', key: entryKey(m.entry), path: r, mod: m.entry.mod_name };
    }
  }

  async function confirmAsk(): Promise<void> {
    const a = ask;
    if (!a) return;
    if (a.kind === 'openAll') {
      ask = null;
      await openAll(a.urls);
      return;
    }
    busy = true;
    try {
      if (a.kind === 'unverified') {
        const r = await commands.manualDownloadsSupplyUnverified(instanceId, a.key, a.path);
        result = r.status === 'ok' ? supplyResult(get(t), [r.data]) : failed(r.error);
      } else {
        const r = await commands.manualDownloadsReplace(instanceId, a.key, a.path);
        if (r.status === 'ok') {
          remember([r.data.row]);
          result = supplyResult(get(t), [r.data.row]);
          if (r.data.receipt) {
            announceRemoval(
              { instanceId },
              r.data.receipt,
              r.data.receipt.items.map((i) => ({ sha1: i.sha1, name: i.name })),
            );
          }
          if (r.data.recycled.length > 0) {
            pushInfo(
              get(t)('modpacks.manual.replace.recycled', { count: r.data.recycled.length }),
              r.data.recycled,
            );
          }
        } else {
          result = failed(r.error);
        }
      }
      noteManualDownloadsChanged();
    } finally {
      ask = null;
      busy = false;
      await load();
    }
  }

  async function openAll(urls: readonly string[]): Promise<void> {
    for (const [i, url] of urls.entries()) {
      if (i > 0) await new Promise((r) => setTimeout(r, OPEN_ALL_GAP_MS));
      await openExternalHttps(url);
    }
  }

  function onOpenAll(): void {
    if (noLinks) return;
    if (links.length > OPEN_ALL_ASK_ABOVE) ask = { kind: 'openAll', urls: links };
    else void openAll(links);
  }

  async function copyLinks(): Promise<void> {
    if (noLinks) return;
    const r = await commands.clipboardWriteText(links.join('\n'));
    if (r.status === 'ok') pushInfo(get(t)('modpacks.manual.copied', { count: links.length }));
    else pushWarning(get(t)('modpacks.manual.copyFailed'), [formatError(r.error)]);
  }

  onMount(() => {
    void load();
    void commands.manualDownloadsDefaultFolder().then((r) => {
      if (r.status === 'ok') defaultFolder = r.data;
    });
    const offFocus = onWindowFocus(() => void load());
    const unlisten = events.modInstalled.listen(() => void load());
    return () => {
      offFocus();
      void unlisten.then((fn) => fn());
    };
  });

  // A drop the window router handed to this dialog.
  $effect(() => {
    const paths = droppedManualDownloads.value;
    if (paths === null) return;
    droppedManualDownloads.value = null;
    void supply(paths);
  });

  // The opt-in watch (spec D4): polls only while this dialog is open and the user turned it on.
  $effect(() => {
    if (!watchOn || folder === null || watchStopped !== null) return;
    const id = setInterval(() => void scan(true), WATCH_EVERY_MS);
    return () => clearInterval(id);
  });
</script>

<Modal
  ariaLabelledby="manual-downloads-title"
  ariaDescribedby="manual-downloads-lead"
  {onClose}
  takesFileDrops
  panelClass="w-[min(720px,94vw)] max-h-[90vh] flex flex-col p-4 gap-3"
  dataTestid="manual-downloads-dialog"
>
  <DialogTitle id="manual-downloads-title" size="lg">
    {$t('modpacks.manual.title', { pack: status?.origin.project_name ?? '' })}
  </DialogTitle>
  <p id="manual-downloads-lead" class="text-sm text-secondary">{$t('modpacks.manual.lead')}</p>

  <div class="flex flex-wrap items-center gap-2">
    <button
      type="button"
      class="btn-secondary btn-sm"
      class:opacity-50={noLinks}
      aria-disabled={noLinks}
      onclick={onOpenAll}>{$t('modpacks.manual.openAll', { count: links.length })}</button
    >
    <button
      type="button"
      class="btn-secondary btn-sm"
      class:opacity-50={noLinks}
      aria-disabled={noLinks}
      onclick={() => void copyLinks()}>{$t('modpacks.manual.copyLinks')}</button
    >
    <button
      type="button"
      class="btn-secondary btn-sm"
      class:opacity-50={cannotScan}
      aria-disabled={cannotScan}
      onclick={() => {
        if (!cannotScan) void scan(false);
      }}>{$t('modpacks.manual.lookInFolder')}</button
    >
  </div>
  <div class="flex flex-col gap-1 text-xs text-secondary">
    {#if folder === null}
      <span>{$t('modpacks.manual.folderUnknown')}</span>
    {:else}
      <span class="truncate">{$t('modpacks.manual.folderLine', { path: folder })}</span>
    {/if}
    <div class="flex flex-wrap items-center gap-3">
      <label class="inline-flex items-center gap-2">
        <input
          type="checkbox"
          data-testid="manual-watch"
          checked={watchOn}
          disabled={folder === null}
          onchange={(e) => {
            watchStopped = null;
            void patchGeneral({ manual_downloads_watch: e.currentTarget.checked });
          }}
        />
        {$t('modpacks.manual.watch')}
      </label>
      <button type="button" class="btn-link text-xs" onclick={() => void changeFolder()}
        >{$t('modpacks.manual.changeFolder')}</button
      >
      {#if watchOn && watchStopped === null && folder !== null}
        <Spinner
          size="sm"
          label={$t('modpacks.manual.watching')}
          labelPlacement="right"
          class="text-muted"
        />
      {/if}
    </div>
    {#if watchStopped}
      <StatusMessage
        message={$t('modpacks.manual.watchStopped')}
        details={[watchStopped]}
        tone="warning"
      />
    {/if}
  </div>

  {#if loadError}
    <StatusMessage message={loadError} tone="danger" />
  {/if}
  <ul
    class="flex-1 min-h-0 overflow-y-auto border border-border-subtle rounded divide-y divide-border-subtle"
  >
    <!-- Keyed with the position too: two entries can share an `entry_key`. -->
    {#each [...pending, ...doneRows] as m, i (`${i}:${entryKey(m.entry)}`)}
      {@const key = entryKey(m.entry)}
      {@const waiting = found[key] ?? null}
      <ManualDownloadRow
        status={m}
        found={waiting?.existing ?? null}
        onUseFile={() => void useFile(m)}
        onReplace={() => {
          if (waiting) {
            ask = {
              kind: 'replace',
              key,
              path: waiting.path,
              existing: waiting.existing,
              mod: m.entry.mod_name,
            };
          }
        }}
      />
    {/each}
  </ul>

  <!-- Stays enabled while a supply runs (focus, DESIGN §8); `chooseFiles` ignores the click. -->
  <FileDropzone
    target="manual-downloads"
    label={$t('modpacks.manual.dropLabel')}
    dragLabel={$t('modpacks.manual.dropDrag')}
    onClick={() => void chooseFiles()}
  />

  <StatusMessage
    message={result.message}
    details={result.details}
    tone={result.tone}
    dataTestid="manual-result"
  />

  <div class="flex items-center justify-between gap-3">
    <span class="text-sm" class:text-success={requiredLeft === 0 && rows.length > 0}>
      {#if requiredLeft === 0 && rows.length > 0}{$t('modpacks.manual.allDone')}{/if}
    </span>
    <button type="button" class="btn-primary btn-sm" onclick={onClose}
      >{$t('modpacks.manual.done')}</button
    >
  </div>
</Modal>

{#if ask}
  {@const a = ask}
  <ConfirmDialog
    title={a.kind === 'openAll'
      ? $t('modpacks.manual.openAllAsk.title', { count: a.urls.length })
      : a.kind === 'unverified'
        ? $t('modpacks.manual.unverifiedAsk.title')
        : $t('modpacks.manual.replaceAsk.title', { existing: a.existing })}
    bodyText={a.kind === 'openAll'
      ? $t('modpacks.manual.openAllAsk.body')
      : a.kind === 'unverified'
        ? $t('modpacks.manual.unverifiedAsk.body', { mod: a.mod })
        : $t('modpacks.manual.replaceAsk.body', { existing: a.existing, mod: a.mod })}
    confirmLabel={a.kind === 'openAll'
      ? $t('modpacks.manual.openAllAsk.action', { count: a.urls.length })
      : a.kind === 'unverified'
        ? $t('modpacks.manual.unverifiedAsk.action')
        : $t('modpacks.manual.action.replace')}
    variant={a.kind === 'replace' ? 'danger' : 'primary'}
    {busy}
    onCancel={() => (ask = null)}
    onConfirm={() => void confirmAsk()}
  />
{/if}

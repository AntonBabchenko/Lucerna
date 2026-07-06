<script lang="ts">
  // Storage settings panel — Task 20 of the v0.5.0 mod browser plan.
  // Rendered inside SettingsModal's Storage tab.
  //
  // Shows the current on-disk size of the shared mod cache (under the
  // launcher's app-data dir) and offers a Clear button. Clearing only
  // wipes the cache jars — installed mods inside instances are not
  // touched (re-installs will re-download from CurseForge / Modrinth).
  //
  // Both IPC calls (modsCacheSizeBytes / modsClearCache) follow the
  // result-status pattern (typedError) — no try/catch around them.
  //
  // The "Data location" block (bottom) lets the user relocate the WHOLE data
  // root (instances, caches, accounts — everything under the app-data dir) to
  // a folder of their choice. See data-location.svelte.ts for the shared
  // status rune (also read by the fallback banner + create/Play gating in
  // +page.svelte) and DataLocationConfirmDialog / DataLocationProgressDialog
  // for the two modals this flow drives.
  import { open as openDirectory } from '@tauri-apps/plugin-dialog';
  import {
    commands,
    events,
    type DataMigrationProgress,
    type LogRetentionPolicy,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { formatSize } from '$lib/format/size';
  import { t } from '$lib/i18n';
  import Spinner from '$lib/ui/Spinner.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import DataLocationConfirmDialog from '$lib/settings/DataLocationConfirmDialog.svelte';
  import DataLocationProgressDialog from '$lib/settings/DataLocationProgressDialog.svelte';

  let bytes = $state<number | null>(null);
  let clearing = $state(false);
  let error = $state<string | null>(null);
  let toast = $state<string | null>(null);

  const DEFAULT_RETENTION: Required<LogRetentionPolicy> = {
    enabled: false,
    max_files: 10,
    max_total_mb: 100,
  };
  let retention = $state<Required<LogRetentionPolicy>>({ ...DEFAULT_RETENTION });
  let retentionError = $state<string | null>(null);
  let retentionSaving = $state(false);

  // Mod-metadata (name/icon/slug) cache TTL in days. 0 = never expire.
  const DEFAULT_TTL_DAYS = 7;
  let modTtlDays = $state<number>(DEFAULT_TTL_DAYS);
  let ttlError = $state<string | null>(null);
  let ttlSaving = $state(false);

  async function loadRetention() {
    const r = await commands.appSettingsGet();
    if (r.status === 'ok') {
      retention = { ...DEFAULT_RETENTION, ...r.data.general.log_retention };
      modTtlDays = r.data.general.mod_metadata_ttl_days ?? DEFAULT_TTL_DAYS;
    } else {
      retentionError = formatError(r.error);
    }
  }

  async function saveRetentionTracked() {
    retentionSaving = true;
    try {
      await saveRetention();
    } finally {
      retentionSaving = false;
    }
  }

  async function saveRetention() {
    retentionError = null;
    const snap = { ...retention }; // snapshot before await
    const cur = await commands.appSettingsGet();
    if (cur.status !== 'ok') {
      retentionError = formatError(cur.error);
      return;
    }
    const next = {
      ...cur.data.general,
      log_retention: {
        enabled: snap.enabled,
        max_files: Number.isFinite(snap.max_files as number)
          ? Math.max(0, Math.trunc(snap.max_files as number))
          : DEFAULT_RETENTION.max_files,
        max_total_mb: Number.isFinite(snap.max_total_mb as number)
          ? Math.max(1, Math.trunc(snap.max_total_mb as number))
          : DEFAULT_RETENTION.max_total_mb,
      },
    };
    const r = await commands.appSettingsSetGeneral(next);
    if (r.status !== 'ok') retentionError = formatError(r.error);
  }

  async function saveTtlTracked() {
    ttlSaving = true;
    try {
      await saveTtl();
    } finally {
      ttlSaving = false;
    }
  }

  async function saveTtl() {
    ttlError = null;
    // Snapshot the owned field before the await (read-modify-write: other
    // panels/fields must survive). Clamp to a non-negative integer; 0 = never.
    const snap = Number.isFinite(modTtlDays)
      ? Math.max(0, Math.trunc(modTtlDays))
      : DEFAULT_TTL_DAYS;
    const cur = await commands.appSettingsGet();
    if (cur.status !== 'ok') {
      ttlError = formatError(cur.error);
      return;
    }
    const next = { ...cur.data.general, mod_metadata_ttl_days: snap };
    const r = await commands.appSettingsSetGeneral(next);
    if (r.status !== 'ok') ttlError = formatError(r.error);
  }

  function fmt(b: number): string {
    if (b < 1024) return `${b} B`;
    if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
    if (b < 1024 * 1024 * 1024) return `${(b / (1024 * 1024)).toFixed(1)} MB`;
    return `${(b / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  async function refresh() {
    const result = await commands.modsCacheSizeBytes();
    if (result.status === 'ok') {
      // The IPC contract types this as `number | null`. Treat null as
      // "unknown" so the UI still has a sensible fallback rather than
      // rendering "null B".
      bytes = result.data ?? 0;
    } else {
      error = formatError(result.error);
    }
  }

  $effect(() => {
    void refresh();
    void loadRetention();
    void dataLocation.init();
  });

  // ── Data-root relocation ────────────────────────────────────────────────
  // pendingTarget: null while no picker/confirm/progress flow is in
  // progress. Set to the picked path (or '' to mean "reset to default")
  // once the user confirms the picker result, opening the confirm dialog.
  let pendingTarget = $state<string | null | 'reset'>(null);
  let migrating = $state(false);
  let migrationError = $state<string | null>(null);
  let migrationProgress = $state<DataMigrationProgress | null>(null);
  let progressUnlisten: (() => void) | null = null;

  async function pickLocation() {
    migrationError = null;
    const picked = await openDirectory({ directory: true });
    if (!picked || typeof picked !== 'string') return;
    pendingTarget = picked;
  }

  function requestReset() {
    migrationError = null;
    pendingTarget = 'reset';
  }

  function cancelPending() {
    pendingTarget = null;
  }

  async function confirmPending() {
    const target = pendingTarget;
    if (target === null) return;
    migrating = true;
    migrationProgress = null;
    migrationError = null;
    progressUnlisten = await events.dataMigrationProgress.listen((event) => {
      migrationProgress = event.payload;
    });
    const newPath = target === 'reset' ? null : target;
    const result = await commands.setDataLocation(newPath);
    // On success setDataLocation never returns (the backend calls
    // app.restart()); reaching here means it failed before that point.
    progressUnlisten?.();
    progressUnlisten = null;
    migrating = false;
    migrationProgress = null;
    pendingTarget = null;
    if (result.status === 'error') {
      migrationError = formatError(result.error);
      await dataLocation.refresh();
    }
  }

  $effect(() => () => {
    progressUnlisten?.();
  });

  async function clear() {
    clearing = true;
    error = null;
    toast = null;
    const result = await commands.modsClearCache();
    if (result.status === 'ok') {
      const freed = result.data ?? 0;
      toast = $t('settings.storage.cleared', { freed: fmt(freed) });
      await refresh();
    } else {
      error = formatError(result.error);
    }
    clearing = false;
  }
</script>

<div>
  <div class="text-sm mb-2">
    {$t('settings.storage.cacheLabel')}
    <span class="font-medium">{bytes === null ? '…' : fmt(bytes)}</span>
  </div>
  <p class="text-xs text-muted mb-3">
    {$t('settings.storage.cacheDescription')}
  </p>

  {#if error}
    <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-2">
      {error}
    </div>
  {/if}
  {#if toast}
    <div class="bg-success-bg border border-success text-success text-sm rounded p-2 mb-2">
      {toast}
    </div>
  {/if}

  <BusyButton
    type="button"
    class="btn-secondary btn-sm"
    busy={clearing}
    disabled={bytes === 0 || bytes === null}
    onclick={clear}
  >
    {$t('settings.storage.clearBtn')}
  </BusyButton>

  <div class="flex flex-col gap-3 border-t mt-4 pt-4">
    <h3 class="font-medium text-sm text-primary">{$t('settings.general.logRetention.title')}</h3>
    {#if retentionError}
      <p class="text-xs text-danger">{retentionError}</p>
    {/if}
    <label class="flex items-start gap-2 cursor-pointer">
      <input
        type="checkbox"
        class="mt-0.5"
        bind:checked={retention.enabled}
        onchange={() => void saveRetentionTracked()}
        data-testid="log-retention-toggle"
      />
      <span class="flex-1">
        <span class="text-sm text-primary">{$t('settings.general.logRetention.enableLabel')}</span>
        <span class="block text-xs text-muted">
          {$t('settings.general.logRetention.enableDescription')}
        </span>
      </span>
    </label>
    <div class="flex flex-wrap items-end gap-4 pl-6">
      {#if retentionSaving}
        <div class="flex items-center text-xs text-secondary">
          <Spinner size="sm" />
        </div>
      {/if}
      <label class="flex flex-col gap-1">
        <span class="text-xs text-primary">{$t('settings.general.logRetention.keepLabel')}</span>
        <input
          type="number"
          min="0"
          class="border rounded px-2 py-1 text-sm w-28"
          bind:value={retention.max_files}
          disabled={!retention.enabled}
          onchange={() => void saveRetentionTracked()}
          data-testid="log-retention-max-files"
        />
      </label>
      <label class="flex flex-col gap-1">
        <span class="text-xs text-primary">{$t('settings.general.logRetention.sizeLabel')}</span>
        <input
          type="number"
          min="1"
          class="border rounded px-2 py-1 text-sm w-28"
          bind:value={retention.max_total_mb}
          disabled={!retention.enabled}
          onchange={() => void saveRetentionTracked()}
          data-testid="log-retention-max-mb"
        />
      </label>
    </div>
  </div>

  <div class="flex flex-col gap-3 border-t mt-4 pt-4">
    <h3 class="font-medium text-sm text-primary">
      {$t('settings.general.modMetadataCache.title')}
    </h3>
    <p class="text-xs text-muted">{$t('settings.general.modMetadataCache.description')}</p>
    {#if ttlError}
      <p class="text-xs text-danger">{ttlError}</p>
    {/if}
    <div class="flex flex-wrap items-end gap-4">
      {#if ttlSaving}
        <div class="flex items-center text-xs text-secondary">
          <Spinner size="sm" />
        </div>
      {/if}
      <label class="flex flex-col gap-1">
        <span class="text-xs text-primary">
          {$t('settings.general.modMetadataCache.ttlLabel')}
        </span>
        <input
          type="number"
          min="0"
          step="1"
          class="border rounded px-2 py-1 text-sm w-28"
          bind:value={modTtlDays}
          onchange={() => void saveTtlTracked()}
          data-testid="mod-metadata-ttl-days"
        />
      </label>
    </div>
    <p class="text-xs text-muted">{$t('settings.general.modMetadataCache.ttlHint')}</p>
  </div>

  <div class="flex flex-col gap-3 border-t mt-4 pt-4">
    <h3 class="font-medium text-sm text-primary">
      {$t('settings.storage.dataLocation.heading')}
    </h3>
    <p class="text-xs text-muted">{$t('settings.storage.dataLocation.description')}</p>

    {#if dataLocation.error}
      <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2">
        {dataLocation.error}
      </div>
    {/if}
    {#if migrationError}
      <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2">
        {migrationError}
      </div>
    {/if}

    {#if dataLocation.status?.fell_back}
      <div
        class="rounded-xl border border-warning-text bg-warning-bg p-3 text-sm text-warning-text"
        role="alert"
      >
        {$t('settings.storage.dataLocation.fallbackNotice', {
          path: dataLocation.status.configured ?? '',
        })}
      </div>
    {/if}

    {#if dataLocation.status}
      <div class="text-sm">
        <span class="text-muted">{$t('settings.storage.dataLocation.currentLabel')}</span>
        <span class="font-mono text-xs selectable ml-1">{dataLocation.status.effective}</span>
      </div>
      <div class="text-sm">
        <span class="text-muted">{$t('settings.storage.dataLocation.sizeLabel')}</span>
        <span class="font-medium ml-1"
          >{formatSize($t, dataLocation.status.data_size_bytes) ||
            $t('format.size.bytes', { n: 0 })}</span
        >
      </div>
    {:else}
      <p class="text-xs text-muted">…</p>
    {/if}

    <div class="flex gap-2">
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={dataLocation.status?.fell_back}
        onclick={() => void pickLocation()}
      >
        {$t('settings.storage.dataLocation.changeBtn')}
      </button>
      {#if dataLocation.status?.configured}
        <button
          type="button"
          class="btn-secondary btn-sm"
          disabled={dataLocation.status?.fell_back}
          onclick={requestReset}
        >
          {$t('settings.storage.dataLocation.resetBtn')}
        </button>
      {/if}
    </div>
  </div>
</div>

{#if pendingTarget !== null && !migrating}
  <DataLocationConfirmDialog
    targetPath={pendingTarget === 'reset' ? null : pendingTarget}
    sizeLabel={formatSize($t, dataLocation.status?.data_size_bytes ?? null) ||
      $t('format.size.bytes', { n: 0 })}
    busy={migrating}
    onCancel={cancelPending}
    onConfirm={() => void confirmPending()}
  />
{/if}

{#if migrating}
  <DataLocationProgressDialog progress={migrationProgress} />
{/if}

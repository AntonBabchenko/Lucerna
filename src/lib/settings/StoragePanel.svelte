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
  import { commands, type LogRetentionPolicy } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';

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

  async function loadRetention() {
    const r = await commands.appSettingsGet();
    if (r.status === 'ok') {
      retention = { ...DEFAULT_RETENTION, ...r.data.general.log_retention };
    } else {
      retentionError = formatError(r.error);
    }
  }

  async function saveRetention() {
    retentionError = null;
    const cur = await commands.appSettingsGet();
    if (cur.status !== 'ok') {
      retentionError = formatError(cur.error);
      return;
    }
    const next = {
      ...cur.data.general,
      log_retention: {
        enabled: retention.enabled,
        max_files: Number.isFinite(retention.max_files as number)
          ? Math.max(0, Math.trunc(retention.max_files as number))
          : DEFAULT_RETENTION.max_files,
        max_total_mb: Number.isFinite(retention.max_total_mb as number)
          ? Math.max(1, Math.trunc(retention.max_total_mb as number))
          : DEFAULT_RETENTION.max_total_mb,
      },
    };
    const r = await commands.appSettingsSetGeneral(next);
    if (r.status !== 'ok') retentionError = formatError(r.error);
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

  <button
    type="button"
    class="btn-secondary btn-sm"
    disabled={clearing || bytes === 0 || bytes === null}
    onclick={clear}
  >
    {$t('settings.storage.clearBtn')}
  </button>

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
        onchange={() => void saveRetention()}
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
      <label class="flex flex-col gap-1">
        <span class="text-xs text-primary">{$t('settings.general.logRetention.keepLabel')}</span>
        <input
          type="number"
          min="0"
          class="border rounded px-2 py-1 text-sm w-28"
          bind:value={retention.max_files}
          disabled={!retention.enabled}
          onchange={() => void saveRetention()}
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
          onchange={() => void saveRetention()}
          data-testid="log-retention-max-mb"
        />
      </label>
    </div>
  </div>
</div>

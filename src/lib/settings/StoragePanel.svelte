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
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';

  let bytes = $state<number | null>(null);
  let clearing = $state(false);
  let error = $state<string | null>(null);
  let toast = $state<string | null>(null);

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
  });

  async function clear() {
    clearing = true;
    error = null;
    toast = null;
    const result = await commands.modsClearCache();
    if (result.status === 'ok') {
      const freed = result.data ?? 0;
      toast = `Cache cleared (${fmt(freed)} freed)`;
      await refresh();
    } else {
      error = formatError(result.error);
    }
    clearing = false;
  }
</script>

<div>
  <div class="text-sm mb-2">
    Mod download cache: <span class="font-medium">{bytes === null ? '…' : fmt(bytes)}</span>
  </div>
  <p class="text-xs text-muted mb-3">
    Clearing only removes cached jars. Installed mods in instances are not affected. Re-installs
    will re-download.
  </p>

  {#if error}
    <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-2">
      {error}
    </div>
  {/if}
  {#if toast}
    <div class="bg-success/10 border border-success text-success text-sm rounded p-2 mb-2">
      {toast}
    </div>
  {/if}

  <button
    type="button"
    class="btn-secondary btn-sm"
    disabled={clearing || bytes === 0 || bytes === null}
    onclick={clear}
  >
    Clear cache
  </button>
</div>

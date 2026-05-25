<script lang="ts">
  import { commands, type World } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import BackupsDialog from '$lib/worlds/BackupsDialog.svelte';
  import DeleteWorldDialog from '$lib/worlds/DeleteWorldDialog.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';

  let {
    instanceId,
    onListChanged,
  }: {
    instanceId: string | null;
    onListChanged: () => void;
  } = $props();

  let worlds = $state<World[]>([]);
  let listError = $state<string | null>(null);
  let loading = $state(false);
  let openMenuFor = $state<string | null>(null);
  let backupsFor = $state<World | null>(null);
  let deleteFor = $state<World | null>(null);

  async function reload() {
    if (!instanceId) {
      worlds = [];
      return;
    }
    loading = true;
    listError = null;
    const r = await commands.listWorlds(instanceId);
    loading = false;
    if (r.status === 'ok') {
      worlds = r.data;
    } else {
      listError = formatError(r.error);
    }
  }

  $effect(() => {
    void instanceId;
    void reload();
  });

  async function onBackupNow(w: World) {
    if (!instanceId) return;
    openMenuFor = null;
    const r = await commands.backupWorld(instanceId, w.folder_name);
    if (r.status === 'ok') {
      pushSuccess(`Backed up ${w.folder_name} (${formatBytes(r.data.size_bytes)})`);
      await reload();
    } else {
      pushWarning(formatError(r.error));
    }
  }

  async function onOpenSavesFolder() {
    if (!instanceId) return;
    const r = await commands.openSavesFolder(instanceId);
    if (r.status !== 'ok') pushWarning(formatError(r.error));
  }

  function formatBytes(n: number | null | undefined): string {
    if (n == null) return '';
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
    return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }

  function relativeTime(ms: number | null | undefined): string {
    if (!ms) return 'never';
    const diff = Date.now() - ms;
    const sec = Math.floor(diff / 1000);
    if (sec < 60) return `${sec}s ago`;
    const min = Math.floor(sec / 60);
    if (min < 60) return `${min}m ago`;
    const hr = Math.floor(min / 60);
    if (hr < 24) return `${hr}h ago`;
    const days = Math.floor(hr / 24);
    return `${days}d ago`;
  }
</script>

<div class="p-3 flex flex-col gap-2" data-testid="worlds-tab">
  {#if !instanceId}
    <p class="text-sm text-neutral-500">Select an instance to view its worlds.</p>
  {:else if loading}
    <p class="text-sm text-neutral-500">Loading worlds…</p>
  {:else if listError}
    <p class="text-sm text-red-700">{listError}</p>
  {:else if worlds.length === 0}
    <p class="text-sm text-neutral-500">No worlds yet. Play Minecraft to create one.</p>
  {:else}
    <ul class="border border-neutral-200 rounded divide-y divide-neutral-200">
      {#each worlds as w (w.folder_name)}
        <li class="flex items-center justify-between gap-2 px-3 py-2">
          <div class="min-w-0">
            <div class="flex items-center gap-2">
              <span class="font-medium truncate">{w.folder_name}</span>
              {#if w.backup_count > 0}
                <span
                  class="text-xs text-amber-800 bg-amber-100 rounded px-1.5 py-0.5"
                  aria-label="{w.backup_count} backups"
                >
                  📦 {w.backup_count}
                </span>
              {/if}
            </div>
            <div class="text-xs text-neutral-500">
              {formatBytes(w.size_bytes)} · {relativeTime(w.modified_unix_ms)}
            </div>
          </div>
          <div class="relative">
            <button
              type="button"
              class="border rounded px-2 py-1 text-sm hover:bg-neutral-50"
              aria-label="Actions for {w.folder_name}"
              onclick={() => (openMenuFor = openMenuFor === w.folder_name ? null : w.folder_name)}
            >
              ⋮
            </button>
            {#if openMenuFor === w.folder_name}
              <div
                class="absolute right-0 mt-1 w-48 bg-white border border-neutral-200 rounded shadow z-10"
                role="menu"
              >
                <button
                  type="button"
                  role="menuitem"
                  class="block w-full text-left px-3 py-2 text-sm hover:bg-neutral-50"
                  onclick={() => void onBackupNow(w)}
                >
                  Back up now
                </button>
                <button
                  type="button"
                  role="menuitem"
                  class="block w-full text-left px-3 py-2 text-sm hover:bg-neutral-50"
                  onclick={() => {
                    openMenuFor = null;
                    backupsFor = w;
                  }}
                >
                  View backups…
                </button>
                <button
                  type="button"
                  role="menuitem"
                  class="block w-full text-left px-3 py-2 text-sm hover:bg-neutral-50 text-red-700"
                  onclick={() => {
                    openMenuFor = null;
                    deleteFor = w;
                  }}
                >
                  Delete world…
                </button>
              </div>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}
  <button
    type="button"
    class="self-start text-sm text-blue-700 hover:underline"
    onclick={() => void onOpenSavesFolder()}
  >
    Open saves folder ↗
  </button>
</div>

{#if backupsFor && instanceId}
  <BackupsDialog
    {instanceId}
    world={backupsFor}
    onClose={() => {
      backupsFor = null;
      void reload();
    }}
    onChanged={() => {
      onListChanged();
      void reload();
    }}
  />
{/if}

{#if deleteFor && instanceId}
  <DeleteWorldDialog
    {instanceId}
    world={deleteFor}
    onClose={() => (deleteFor = null)}
    onDeleted={() => {
      deleteFor = null;
      onListChanged();
      void reload();
    }}
  />
{/if}

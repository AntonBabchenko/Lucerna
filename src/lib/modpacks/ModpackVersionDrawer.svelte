<script lang="ts">
  import { commands } from '$lib/ipc/bindings';
  import type { ModpackHit, ModpackVersionEntry } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';

  // Right-side drawer that lists the Modrinth versions for a picked
  // modpack. Clicking Install resolves the chosen version's .mrpack to a
  // temp path (via `modpack_fetch_to_temp` on the Rust side) and hands
  // the path back to the parent, which then runs the picker dialog +
  // import pipeline (Tasks 8–9).

  let {
    hit,
    onClose,
    onInstall,
  }: {
    hit: ModpackHit;
    onClose: () => void;
    onInstall: (tempPath: string, versionId: string) => void;
  } = $props();

  let versions = $state<ModpackVersionEntry[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let downloading = $state(false);

  $effect(() => {
    void hit.project_id;
    (async () => {
      loading = true;
      error = null;
      const r = await commands.modpackGetVersions(hit.project_id);
      if (r.status === 'ok') {
        versions = r.data;
      } else {
        error = formatError(r.error);
      }
      loading = false;
    })();
  });

  async function install(versionId: string) {
    downloading = true;
    try {
      const result = await commands.modpackFetchToTemp(hit.project_id, versionId);
      if (result.status === 'ok') onInstall(result.data, versionId);
      else error = formatError(result.error);
    } finally {
      downloading = false;
    }
  }
</script>

<!--
  Drawer container is a <div role="dialog"> (not <aside>) so the
  interactive `dialog` role is on an element that accepts it; <aside>
  is a landmark and svelte-check warns about marking it interactive.
  Matches the v0.5.0 sub-3 ModDetailDrawer convention.
-->
<div
  class="fixed top-0 right-0 h-full w-96 bg-white shadow-xl border-l overflow-y-auto"
  role="dialog"
  aria-label="Modpack version list"
>
  <header class="p-4 border-b flex items-center">
    <h3 class="font-semibold flex-1">{hit.title}</h3>
    <button
      type="button"
      class="text-neutral-500 hover:text-neutral-900"
      onclick={onClose}
      aria-label="Close"
    >
      ×
    </button>
  </header>
  <div class="p-4">
    {#if loading}
      <div class="text-sm text-neutral-500">Loading versions...</div>
    {:else if error}
      <div class="text-sm text-red-600">{error}</div>
    {:else}
      <ul class="space-y-2">
        {#each versions as v (v.id)}
          <li class="p-2 border rounded text-sm">
            <div class="flex items-center">
              <div class="flex-1 min-w-0">
                <div class="font-medium truncate">{v.name}</div>
                <div class="text-xs text-neutral-500">
                  MC {v.game_versions.join(', ')} · {v.loaders.join(', ')}
                </div>
              </div>
              <button
                type="button"
                class="ml-2 px-2 py-1 text-xs bg-blue-600 text-white rounded disabled:bg-neutral-300"
                disabled={downloading}
                onclick={() => install(v.id)}
              >
                Install
              </button>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

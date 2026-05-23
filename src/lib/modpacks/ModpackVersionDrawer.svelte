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

  // A pack whose author disabled third-party distribution cannot be
  // installed in-app. The flag arrives on the hit (advance hint); the
  // per-file check in modpack_fetch_to_temp catches the null-unknown
  // case at install time and raises installBlocked.
  let installBlocked = $state(false);
  let blocked = $derived(hit.distribution_allowed === false || installBlocked);

  function openOnCurseForge() {
    void import('@tauri-apps/plugin-opener').then((m) =>
      m.openUrl(`https://www.curseforge.com/minecraft/modpacks/${hit.slug}`),
    );
  }

  $effect(() => {
    void hit.project_id;
    (async () => {
      loading = true;
      error = null;
      const r = await commands.modpackGetVersions(hit.source, hit.project_id);
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
      const result = await commands.modpackFetchToTemp(hit.source, hit.project_id, versionId);
      if (result.status === 'ok') {
        onInstall(result.data, versionId);
      } else if (result.error.kind === 'modpack_cf_distribution_disabled') {
        installBlocked = true;
      } else {
        error = formatError(result.error);
      }
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
    {#if blocked}
      <div class="text-sm text-neutral-700">
        <p class="mb-3">
          The author of this CurseForge modpack disabled third-party launcher downloads, so it
          cannot be installed automatically. Open it on CurseForge to download the <code>.zip</code
          >, then import it with the drag-and-drop box above.
        </p>
        <button
          type="button"
          class="px-3 py-1.5 text-sm bg-blue-600 text-white rounded"
          onclick={openOnCurseForge}
        >
          Open on CurseForge ↗
        </button>
      </div>
    {:else if loading}
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

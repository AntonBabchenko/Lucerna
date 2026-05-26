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
    mcFilter = null,
    onClose,
    onInstall,
  }: {
    hit: ModpackHit;
    // MC version filter forwarded from the browse toolbar. When set,
    // hide pack versions whose `game_versions` don't include it — the
    // user came in with that filter applied to the grid, so the drawer
    // should reflect the same scope. Null = show every version.
    mcFilter?: string | null;
    onClose: () => void;
    onInstall: (tempPath: string, versionId: string) => void;
  } = $props();

  let versions = $state<ModpackVersionEntry[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let downloading = $state(false);

  const visibleVersions = $derived(
    mcFilter ? versions.filter((v) => v.game_versions.includes(mcFilter)) : versions,
  );

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

<div class="fixed inset-0 z-30 flex items-center justify-center">
  <button type="button" class="absolute inset-0 bg-black/30" aria-label="Close" onclick={onClose}
  ></button>
  <div
    class="relative bg-surface rounded shadow-lg max-w-2xl w-full max-h-[85vh] overflow-y-auto m-4"
    role="dialog"
    aria-modal="true"
    aria-label="Modpack version list"
  >
    <header class="p-4 border-b flex items-center">
      <h3 class="font-semibold text-primary flex-1">{hit.title}</h3>
      <button
        type="button"
        class="text-muted hover:text-primary"
        onclick={onClose}
        aria-label="Close"
      >
        ×
      </button>
    </header>
    <div class="p-4">
      {#if blocked}
        <div class="text-sm text-secondary">
          <p class="mb-3">
            The author of this CurseForge modpack disabled third-party launcher downloads, so it
            cannot be installed automatically. Open it on CurseForge to download the
            <code>.zip</code>, then import it with the drag-and-drop box above.
          </p>
          <button
            type="button"
            class="px-3 py-1.5 text-sm bg-accent text-white rounded"
            onclick={openOnCurseForge}
          >
            Open on CurseForge ↗
          </button>
        </div>
      {:else if loading}
        <div class="text-sm text-muted">Loading versions...</div>
      {:else if error}
        <div class="text-sm text-danger">{error}</div>
      {:else if visibleVersions.length === 0}
        <div class="text-sm text-muted">
          {#if mcFilter}
            No versions for MC {mcFilter}. Clear the MC filter in the search to see all.
          {:else}
            No versions available.
          {/if}
        </div>
      {:else}
        <ul class="space-y-2">
          {#each visibleVersions as v (v.id)}
            <li class="p-2 border rounded text-sm">
              <div class="flex items-center">
                <div class="flex-1 min-w-0">
                  <div class="font-medium truncate">{v.name}</div>
                  <div class="text-xs text-muted">
                    MC {v.game_versions.join(', ')} · {v.loaders.join(', ')}
                  </div>
                </div>
                <button
                  type="button"
                  class="ml-2 px-2 py-1 text-xs bg-accent text-white rounded disabled:bg-muted"
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
</div>

<script lang="ts">
  import { Channel } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { commands, events } from '$lib/ipc/bindings';
  import type {
    InstalledMod,
    InstanceWithStatus,
    ModpackProgress,
    ModpackStatus,
    ModpackUpdateDiff,
    ModpackVersionEntry,
    ModSource,
    PackOriginFile,
    ProgressTick,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { drawerCache } from './drawer-cache';
  import ModpackUpdateDialog from './ModpackUpdateDialog.svelte';

  // Right-side drawer that surfaces the metadata captured at import time
  // for a pack-originated instance. Mirrors ModpackVersionDrawer's
  // positioning, header + close button, and the `role="dialog"` on a
  // <div> (not an <aside>) so svelte-check is happy with the interactive
  // role on a non-landmark element.
  //
  // Source link rendering: a pack imported from the Modrinth Browse
  // flow lands with `mrpack_project_id` + `mrpack_source = 'modrinth'`
  // already populated, so the drawer can deep-link back to the
  // Modrinth (or CurseForge) project page without another API hop.
  // Drag-drop imports usually leave these null (the v0.5.0 sub-4 P1
  // orchestrator does a Modrinth-side auto-lookup by version_id, but
  // not every .mrpack hits that path) — in that case the link is
  // simply hidden.
  //
  // Mod list: hits `mods_list_installed`, which reconciles the on-disk
  // jars with the registry. Even mods that the user added by hand after
  // the import (drop into the instance's mods folder) show up here, so
  // the drawer stays accurate over time, not just immediately after
  // import.
  //
  // Provenance badges (bundle 2): `modpack_status` returns the frozen
  // pack-origin snapshot. We compute `originShas` once on each refresh
  // and use it to badge each installed row:
  //   - 📦 pack    — sha1 IS in originShas
  //   - + user    — sha1 NOT in originShas, mod has a source (added
  //                 via the Mod browser after import)
  //   - ? manual  — sha1 NOT in originShas, mod has no source (user
  //                 dropped a jar into mods/ by hand)
  //
  // Removed-from-pack section (bundle 2): if any sha in `origin.files`
  // is missing from `installed_shas`, render it under the installed
  // list with a Restore button that calls modpack_restore_file.
  //
  // Project-name enrichment (bundle 2): mirrors the workaround in
  // src/lib/mods/InstalledModsView.svelte — the registry stores a
  // version-shaped string in `m.name` for some mods (the Modrinth
  // version manifest doesn't always carry the display title). We
  // re-fetch via mods_project per installed-with-source mod, in
  // parallel, store the canonical title in a Map<sha1, name>, and use
  // it as the display name with fallback to `m.name`. Failures here
  // are silent — a transient lookup error keeps the registry name.
  //
  // Delete confirm overlay copy spells out the consequences (worlds,
  // configs, screenshots) — matches the ManageInstancesModal delete
  // dialog wording so the user gets the same warning regardless of
  // where they trigger the delete from. LastInstance errors from the
  // backend are surfaced via formatError; the button stays clickable
  // because we don't know the instance count from inside this drawer.

  let {
    inst,
    onClose,
    onOpenInstance,
    onDeleted,
    onUpdated,
  }: {
    inst: InstanceWithStatus;
    onClose: () => void;
    onOpenInstance: (id: string) => void;
    onDeleted: () => void;
    onUpdated?: () => void;
  } = $props();

  let mods = $state<InstalledMod[] | null>(null);
  let status = $state<ModpackStatus | null>(null);
  let nameMap = $state<Map<string, string>>(new Map());
  let restoreError = $state<string | null>(null);
  let deleting = $state(false);
  let deleteError = $state<string | null>(null);

  let updateAvailable = $state<ModpackVersionEntry | null>(null);
  let updateDiff = $state<ModpackUpdateDiff | null>(null);
  let updateTempPath = $state<string | null>(null);
  let updating = $state(false);
  let updateError = $state<string | null>(null);

  const originShas = $derived(
    new Set((status?.origin.files ?? []).map((f) => f.sha1.toLowerCase())),
  );

  // A pack file's category, from its install_path prefix. Drives the
  // per-type drawer sections. Anything that is not mods/resourcepacks/
  // shaderpacks (config/*, options.txt, …) groups under "configs".
  type AssetCat = 'resourcepacks' | 'shaderpacks' | 'configs';
  function assetCat(installPath: string): AssetCat {
    if (installPath.startsWith('resourcepacks/')) return 'resourcepacks';
    if (installPath.startsWith('shaderpacks/')) return 'shaderpacks';
    return 'configs';
  }

  // install_paths currently flagged removed — excluded from the present
  // sections (they render in "Removed from pack" instead).
  const removedPaths = $derived(new Set((status?.removed_files ?? []).map((f) => f.install_path)));

  // Present (non-removed) pack files of one category. Mods are excluded —
  // they have their own disk-reality-driven section above.
  function presentAssets(cat: AssetCat): PackOriginFile[] {
    return (status?.origin.files ?? []).filter(
      (f) =>
        !f.install_path.startsWith('mods/') &&
        assetCat(f.install_path) === cat &&
        !removedPaths.has(f.install_path),
    );
  }

  let configsExpanded = $state(false);

  $effect(() => {
    void inst.id;
    void load();
    void checkForUpdates();
  });

  // While the drawer is open, a mod installed or removed elsewhere
  // (e.g. a drag-drop local install on the Mods tab) can resolve or
  // un-resolve a "Mods to install manually" entry. Refresh silently so
  // the section and provenance badges stay live without a loading flash.
  onMount(() => {
    const unlisten = [
      events.modInstalled.listen(() => void load(true)),
      events.modUninstalled.listen(() => void load(true)),
    ];
    return () => {
      for (const u of unlisten) void u.then((fn) => fn());
    };
  });

  async function load(silent = false) {
    // Seed from the session cache so a reopened drawer renders instantly
    // instead of flashing "Loading…"; load() then revalidates below. A
    // silent refresh (fired by a mod-install event) likewise skips the
    // loading reset. Only a first-ever open with no cache entry shows
    // the "Loading…" state.
    const cached = drawerCache.get(inst.id);
    if (cached) {
      mods = cached.mods;
      status = cached.status;
      nameMap = cached.nameMap;
    } else if (!silent) {
      mods = null;
      status = null;
      nameMap = new Map();
    }
    restoreError = null;

    const [listR, statusR] = await Promise.all([
      commands.modsListInstalled(inst.id),
      commands.modpackStatus(inst.id),
    ]);

    if (listR.status === 'ok') {
      mods = listR.data;
    } else {
      // Fall through to "No mods installed" on error; the drawer's
      // primary purpose is pack metadata, not a mods debugger.
      mods = [];
    }
    if (statusR.status === 'ok') {
      status = statusR.data;
    } else {
      status = null;
    }

    // Project-name enrichment. Same shape as InstalledModsView: fetch
    // ModProject per installed mod with a source, drop errors silently,
    // build a map sha1 -> canonical name. Concurrent across all rows.
    // Always reassign `nameMap` (an empty map when there are no mods) so
    // a silent refresh down to zero mods can't leave stale entries.
    const next = new Map<string, string>();
    await Promise.all(
      mods.map(async (m) => {
        if (m.source == null || m.project_id == null) return;
        const p = await commands.modsProject(m.source as ModSource, m.project_id);
        if (p.status === 'ok') {
          next.set(m.sha1, p.data.summary.name);
        }
      }),
    );
    nameMap = next;
    // Store shallow copies of the collections so a later in-place
    // mutation of this drawer's own `mods` / `nameMap` can't reach back
    // and corrupt the cached snapshot. `status` is a read-only IPC value.
    drawerCache.set(inst.id, { mods: [...mods], status, nameMap: new Map(nameMap) });
  }

  async function checkForUpdates() {
    updateError = null;
    const r = await commands.modpackCheckUpdate(inst.id);
    if (r.status === 'ok') {
      updateAvailable = r.data;
    } else {
      updateError = formatError(r.error);
    }
  }

  // Fetch the new .mrpack + compute the diff, then open the dialog.
  async function openUpdateDialog() {
    if (!updateAvailable || !inst.mrpack_project_id) return;
    updateError = null;
    const fetched = await commands.modpackFetchToTemp(inst.mrpack_source ?? 'modrinth', inst.mrpack_project_id, updateAvailable.id);
    if (fetched.status === 'error') {
      updateError = formatError(fetched.error);
      return;
    }
    updateTempPath = fetched.data;
    const d = await commands.modpackComputeUpdate(inst.id, updateTempPath);
    if (d.status === 'error') {
      updateError = formatError(d.error);
      return;
    }
    updateDiff = d.data;
  }

  async function applyUpdate() {
    if (!updateTempPath || !updateAvailable) return;
    const newVersionId = updateAvailable.id;
    updateDiff = null;
    updating = true;
    updateError = null;
    const phaseChannel = new Channel<ModpackProgress>();
    const tickChannel = new Channel<ProgressTick>();
    const r = await commands.modpackApplyUpdate(
      inst.id,
      updateTempPath,
      newVersionId,
      phaseChannel,
      tickChannel,
    );
    updating = false;
    if (r.status === 'error') {
      updateError = formatError(r.error);
      return;
    }
    updateAvailable = null;
    updateTempPath = null;
    await load();
    onUpdated?.();
  }

  let reimporting = $state(false);
  async function reimportPackFiles() {
    reimporting = true;
    restoreError = null;
    const phaseChannel = new Channel<ModpackProgress>();
    const r = await commands.modpackReimportOverrides(inst.id, phaseChannel);
    reimporting = false;
    if (r.status === 'error') {
      restoreError = formatError(r.error);
      return;
    }
    await load();
    onUpdated?.();
  }

  function sourceUrl(i: InstanceWithStatus): string | null {
    if (!i.mrpack_project_id || !i.mrpack_source) return null;
    if (i.mrpack_source === 'modrinth')
      return `https://modrinth.com/modpack/${i.mrpack_project_id}`;
    if (i.mrpack_source === 'curseforge')
      return `https://www.curseforge.com/projects/${i.mrpack_project_id}`;
    return null;
  }

  function sourceLabel(src: ModSource | null): string {
    if (src === 'modrinth') return 'Modrinth';
    if (src === 'curseforge') return 'CurseForge';
    return '';
  }

  function formatBadge(src: ModSource | null): string {
    if (src === 'modrinth') return 'Modrinth .mrpack';
    if (src === 'curseforge') return 'CurseForge .zip';
    return '.mrpack';
  }

  function displayName(m: InstalledMod): string {
    return nameMap.get(m.sha1) ?? m.name;
  }

  type Provenance = 'pack' | 'user' | 'manual';

  function provenance(m: InstalledMod): Provenance {
    if (originShas.has(m.sha1.toLowerCase())) return 'pack';
    if (m.source != null) return 'user';
    return 'manual';
  }

  async function restore(f: PackOriginFile) {
    restoreError = null;
    const r = await commands.modpackRestoreFile(inst.id, f.sha1);
    if (r.status === 'error') {
      restoreError = formatError(r.error);
      return;
    }
    // Re-load both lists so the file moves from removed → installed.
    await load();
  }

  async function confirmDelete() {
    deleteError = null;
    const r = await commands.deleteInstance(inst.id);
    if (r.status === 'ok') {
      deleting = false;
      onDeleted();
    } else {
      deleteError = formatError(r.error);
    }
  }
</script>

<div
  class="fixed top-0 right-0 h-full w-96 bg-white shadow-xl border-l overflow-y-auto flex flex-col"
  role="dialog"
  aria-label="Imported pack details"
  data-testid="imported-detail-drawer"
>
  <header class="p-4 border-b flex items-start gap-3">
    <div class="text-2xl leading-none flex-shrink-0">📦</div>
    <div class="flex-1 min-w-0">
      <h3 class="font-semibold truncate">{inst.mrpack_name}</h3>
      <div class="text-xs text-neutral-500 truncate">
        v{inst.mrpack_version} · {formatBadge(inst.mrpack_source)}
      </div>
    </div>
    <button
      type="button"
      class="text-neutral-500 hover:text-neutral-900 flex-shrink-0"
      onclick={onClose}
      aria-label="Close"
    >
      ×
    </button>
  </header>

  {#if inst.mrpack_summary}
    <div class="px-4 pt-3 pb-2 text-sm text-neutral-700" data-testid="imported-detail-summary">
      {inst.mrpack_summary}
    </div>
  {/if}

  {#if sourceUrl(inst)}
    <div class="px-4 pb-3">
      <a
        target="_blank"
        rel="noopener"
        href={sourceUrl(inst) ?? ''}
        class="text-blue-600 hover:underline text-sm"
        data-testid="imported-detail-source-link"
      >
        Open on {sourceLabel(inst.mrpack_source)} ↗
      </a>
    </div>
  {/if}

  {#if updateError}
    <div class="px-4 pb-2 text-xs text-red-700" data-testid="imported-detail-update-error">
      {updateError}
    </div>
  {/if}
  {#if updating}
    <div class="px-4 pb-3 text-sm text-blue-700" data-testid="imported-detail-updating">Updating…</div>
  {:else if updateAvailable}
    <div class="px-4 pb-3">
      <div class="flex items-center gap-2 bg-blue-50 border border-blue-200 rounded p-2 text-sm">
        <span class="flex-1 text-blue-900">Update available → {updateAvailable.version_number}</span>
        <button
          type="button"
          class="text-xs px-2 py-0.5 rounded bg-blue-600 text-white hover:bg-blue-700"
          onclick={() => void openUpdateDialog()}
          data-testid="imported-detail-update-button"
        >
          Update
        </button>
      </div>
    </div>
  {/if}

  <div class="flex-1 overflow-y-auto px-4 pb-4">
    <h4 class="font-medium text-sm text-neutral-700 mt-2 mb-2">Mods</h4>
    {#if mods === null}
      <div class="text-sm text-neutral-500" data-testid="imported-detail-mods-loading">
        Loading…
      </div>
    {:else if mods.length === 0}
      <div class="text-sm text-neutral-500" data-testid="imported-detail-mods-empty">
        No mods installed.
      </div>
    {:else}
      <ul class="space-y-1" data-testid="imported-detail-mods-list">
        {#each mods as m (m.sha1)}
          {@const prov = provenance(m)}
          <li class="flex items-center gap-2 text-sm py-1">
            <div
              class="w-2 h-2 rounded-full flex-shrink-0"
              class:bg-blue-500={m.enabled}
              class:bg-neutral-300={!m.enabled}
              aria-hidden="true"
            ></div>
            {#if prov === 'pack'}
              <span
                class="text-[10px] font-medium px-1.5 py-0.5 rounded bg-blue-50 text-blue-700 flex-shrink-0"
                title="from pack"
                data-testid="mod-badge-pack-{m.sha1}"
              >
                📦 pack
              </span>
            {:else if prov === 'user'}
              <span
                class="text-[10px] font-medium px-1.5 py-0.5 rounded bg-purple-50 text-purple-700 flex-shrink-0"
                title="added via Mod browser"
                data-testid="mod-badge-user-{m.sha1}"
              >
                + added
              </span>
            {:else}
              <span
                class="text-[10px] font-medium px-1.5 py-0.5 rounded bg-neutral-100 text-neutral-600 flex-shrink-0"
                title="manually placed"
                data-testid="mod-badge-manual-{m.sha1}"
              >
                ? manual
              </span>
            {/if}
            <span class="truncate flex-1">{displayName(m)}</span>
            {#if !m.enabled}
              <span
                class="text-[10px] font-medium px-1.5 py-0.5 rounded bg-neutral-100 text-neutral-500 flex-shrink-0"
              >
                disabled
              </span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    {#if status}
      {@const resourcepacks = presentAssets('resourcepacks')}
      {@const shaderpacks = presentAssets('shaderpacks')}
      {@const configs = presentAssets('configs')}

      {#if resourcepacks.length > 0}
        <h4 class="font-medium text-sm text-neutral-700 mt-5 mb-2">
          Resource packs ({resourcepacks.length})
        </h4>
        <ul class="space-y-1" data-testid="imported-detail-resourcepacks">
          {#each resourcepacks as f (f.install_path)}
            <li class="flex items-center gap-2 text-sm py-1">
              <span class="truncate flex-1">{f.name}</span>
            </li>
          {/each}
        </ul>
      {/if}

      {#if shaderpacks.length > 0}
        <h4 class="font-medium text-sm text-neutral-700 mt-5 mb-2">
          Shader packs ({shaderpacks.length})
        </h4>
        <ul class="space-y-1" data-testid="imported-detail-shaderpacks">
          {#each shaderpacks as f (f.install_path)}
            <li class="flex items-center gap-2 text-sm py-1">
              <span class="truncate flex-1">{f.name}</span>
            </li>
          {/each}
        </ul>
      {/if}

      {#if configs.length > 0}
        <button
          type="button"
          class="font-medium text-sm text-neutral-700 mt-5 mb-2 flex items-center gap-1 hover:text-neutral-900"
          onclick={() => (configsExpanded = !configsExpanded)}
          data-testid="imported-detail-configs-toggle"
        >
          <span>{configsExpanded ? '▾' : '▸'}</span>
          Configs ({configs.length})
        </button>
        {#if configsExpanded}
          <ul class="space-y-1" data-testid="imported-detail-configs">
            {#each configs as f (f.install_path)}
              <li class="flex items-center gap-2 text-sm py-1">
                <span class="truncate flex-1 text-neutral-600">{f.install_path}</span>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}
    {/if}

    {#if status && status.removed_files.length > 0}
      <div class="mt-5" data-testid="imported-detail-removed-section">
        <h4 class="font-medium text-sm text-neutral-700 mb-2">
          Removed from pack ({status.removed_files.length})
        </h4>
        {#if restoreError}
          <p class="text-xs text-red-700 mb-2" data-testid="imported-detail-restore-error">
            {restoreError}
          </p>
        {/if}
        <ul class="space-y-1">
          {#each status.removed_files as f (f.sha1)}
            <li
              class="flex items-center gap-2 text-sm py-1 px-2 rounded bg-red-50 border border-red-100"
            >
              <span class="truncate flex-1 line-through text-neutral-700">{f.name}</span>
              {#if f.url === ''}
                <!-- Bundled mod from overrides/mods/ — no network source
                     to re-fetch from. Disable Restore and explain why on
                     hover; the user's only recovery path is re-importing
                     the .mrpack. -->
                <button
                  type="button"
                  class="text-xs px-2 py-0.5 rounded border border-neutral-200 text-neutral-400 cursor-not-allowed flex-shrink-0"
                  disabled
                  title="Bundled inside the .mrpack — cannot restore automatically. Re-import the pack to recover."
                  data-testid="imported-detail-restore-{f.sha1}"
                >
                  Restore
                </button>
              {:else}
                <button
                  type="button"
                  class="text-xs px-2 py-0.5 rounded border border-red-200 text-red-700 hover:bg-red-100 flex-shrink-0"
                  onclick={() => void restore(f)}
                  data-testid="imported-detail-restore-{f.sha1}"
                >
                  Restore
                </button>
              {/if}
            </li>
          {/each}
        </ul>
        {#if status.removed_files.some((f) => f.url === '')}
          <button
            type="button"
            class="mt-2 text-xs px-2 py-1 rounded border border-neutral-300 hover:bg-neutral-50 disabled:opacity-50"
            onclick={() => void reimportPackFiles()}
            disabled={reimporting}
            data-testid="imported-detail-reimport"
          >
            {reimporting ? 'Re-importing…' : 'Re-import pack files'}
          </button>
        {/if}
      </div>
    {/if}

    {#if status && status.missing_mods.length > 0}
      <div class="mt-5" data-testid="imported-detail-missing-section">
        <h4 class="font-medium text-sm text-neutral-700 mb-2">
          Pack mods needing attention ({status.missing_mods.filter((m) => m.state !== 'installed').length})
        </h4>
        <p class="text-xs text-neutral-500 mb-2">
          The pack author disabled automatic downloads for these. Download each
          from its source and drop the jar onto the Mods tab.
        </p>
        <ul class="space-y-1">
          {#each status.missing_mods as m (m.entry.mod_name + '|' + m.entry.filename)}
            {@const isInstalled = m.state === 'installed'}
            {@const isDifferentVersion = m.state === 'different_version'}
            <li
              class="flex items-center gap-2 text-sm py-1 px-2 rounded border"
              class:bg-amber-50={!isInstalled}
              class:border-amber-100={!isInstalled}
              class:bg-green-50={isInstalled}
              class:border-green-100={isInstalled}
            >
              <!-- ✓ when the mod is present at all (installed or a
                   different version); ⚠ only when truly missing — so
                   "different version" is not mistaken for "missing". -->
              <span class="flex-shrink-0" aria-hidden="true">
                {m.state === 'missing' ? '⚠' : '✓'}
              </span>
              <span class="truncate flex-1" class:text-neutral-500={isInstalled}>
                {m.entry.mod_name}
                {#if isDifferentVersion}
                  <span class="text-neutral-500 text-xs"> — different version than the pack — may be incompatible</span>
                {/if}
              </span>
              {#if !isInstalled}
                <span
                  class="text-[10px] px-1.5 py-0.5 rounded bg-neutral-100 text-neutral-600 flex-shrink-0"
                >
                  {m.entry.reason === 'distribution_disabled'
                    ? 'Distribution disabled'
                    : 'Host not allowed'}
                </span>
              {/if}
              {#if !isInstalled && m.entry.manual_action_url}
                <a
                  href={m.entry.manual_action_url}
                  target="_blank"
                  rel="noopener"
                  class="text-blue-600 hover:underline text-xs flex-shrink-0"
                >
                  Open ↗
                </a>
              {/if}
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  </div>

  <footer class="p-4 border-t flex justify-end gap-2 sticky bottom-0 bg-white flex-shrink-0">
    <button
      type="button"
      class="px-3 py-1.5 text-sm rounded text-red-600 hover:bg-red-50 border border-transparent"
      onclick={() => (deleting = true)}
      data-testid="imported-detail-delete"
    >
      Delete pack
    </button>
    <button
      type="button"
      class="px-3 py-1.5 text-sm rounded bg-blue-600 text-white hover:bg-blue-700"
      onclick={() => onOpenInstance(inst.id)}
      data-testid="imported-detail-open"
    >
      Open instance
    </button>
  </footer>
</div>

{#if updateDiff}
  <ModpackUpdateDialog
    diff={updateDiff}
    onCancel={() => {
      updateDiff = null;
      updateTempPath = null;
    }}
    onConfirm={() => void applyUpdate()}
  />
{/if}

{#if deleting}
  <div
    class="fixed inset-0 bg-black/50 flex items-center justify-center z-[60]"
    role="dialog"
    aria-modal="true"
    aria-label="Delete pack confirmation"
  >
    <div class="bg-white rounded-lg shadow-xl w-[440px] p-5 flex flex-col gap-3">
      <h3 class="font-semibold text-base">Delete pack and its instance?</h3>
      <p class="text-sm text-neutral-700">
        Worlds, mods, configs, screenshots for this instance will be permanently lost. This cannot
        be undone.
      </p>
      {#if deleteError}
        <p class="text-xs text-red-700" data-testid="imported-detail-delete-error">
          {deleteError}
        </p>
      {/if}
      <div class="flex justify-end gap-2 mt-2">
        <button
          type="button"
          class="border rounded px-3 py-1 text-sm"
          onclick={() => {
            deleting = false;
            deleteError = null;
          }}
          data-testid="imported-detail-delete-cancel"
        >
          Cancel
        </button>
        <button
          type="button"
          class="bg-red-600 text-white rounded px-3 py-1 text-sm hover:bg-red-700"
          onclick={confirmDelete}
          data-testid="imported-detail-delete-confirm"
        >
          Delete
        </button>
      </div>
    </div>
  </div>
{/if}

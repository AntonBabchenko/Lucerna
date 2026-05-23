<script lang="ts">
  import {
    commands,
    events,
    type InstalledMod,
    type LoaderKind,
    type ModSource,
    type ModSummary,
    type ModUpdateCheck,
    type ModVersion,
    type PackOriginSummary,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { settingsOpen } from '$lib/settings/state.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { onDestroy, onMount } from 'svelte';
  import CurseForgeKeyBanner from './CurseForgeKeyBanner.svelte';
  import ModCard from './ModCard.svelte';
  import ModDetailDrawer from './ModDetailDrawer.svelte';
  import { updateCheckCache } from './update-check-cache';

  // The Installed pane of ModBrowserTab. Renders the same ModCard
  // component the Browse pane uses, so the UI is consistent — same
  // icons, layout, Disable/Enable + Uninstall affordances. The only
  // difference is the list is filtered to mods currently installed in
  // the active instance.
  //
  // Each row pairs a ModSummary (fetched lazily from the platform per
  // installed mod's project_id) with the InstalledMod record from
  // {instance}/ftlauncher/installed-mods.json. Manual mods (jars the
  // user dropped into the mods folder by hand — source: null) render a
  // degraded row with no icon and the filename as the title, since
  // they have no platform metadata to fetch.
  //
  // Event listeners are belt-and-suspenders: the action handlers also
  // call refresh() directly after each IPC since the typed
  // events.modX.listen channels don't always match the backend's
  // string-emit channel names.

  let {
    instanceId,
    mcVersion,
    loader,
  }: {
    instanceId: string | null;
    mcVersion: string | null;
    loader: LoaderKind | null;
  } = $props();

  type Row = {
    summary: ModSummary | null;
    installed: InstalledMod;
  };

  let rows = $state<Row[]>([]);
  let filter = $state('');
  let enabledFilter = $state<'all' | 'enabled' | 'disabled'>('all');
  let sortBy = $state<'name-asc' | 'name-desc' | 'recent' | 'source'>('name-asc');
  let error = $state<string | null>(null);
  let loading = $state(false);
  let busy = $state(false);

  // Open the version drawer when the user clicks a card body. The
  // drawer shows the full version list with the installed version
  // highlighted; clicking another version triggers a swap (uninstall
  // current + install new) via switchVersion below.
  let drawerRow = $state<Row | null>(null);

  // Mod-update check. `updateChecks` is keyed by installed-mod sha1 and
  // holds only eligible (non-pack, platform-installed) mods. Seeded from
  // the per-instance session cache so reopening the tab is instant; the
  // "Check for updates" button forces a fresh check. `packSummary` feeds
  // the per-row "from modpack" chip and is loaded on every refresh().
  let updateChecks = $state<Map<string, ModUpdateCheck>>(new Map());
  let checking = $state(false);
  let packSummary = $state<PackOriginSummary | null>(null);
  let showCfBanner = $state(false);

  const updateCount = $derived(
    [...updateChecks.values()].filter((c) => c.state.kind === 'update_available').length,
  );

  function rowDisplayName(r: Row): string {
    return r.summary?.name ?? r.installed.name;
  }

  // Sort the unfiltered rows according to the user's choice. Default
  // is name-asc (predictable, and a version swap doesn't shuffle the
  // mod's slot since uninstall+install appends to installed-mods.json
  // under the hood).
  const sorted = $derived.by(() => {
    const xs = [...rows];
    const nameLower = (r: Row) => rowDisplayName(r).toLowerCase();
    switch (sortBy) {
      case 'name-asc':
        return xs.sort((a, b) => nameLower(a).localeCompare(nameLower(b)));
      case 'name-desc':
        return xs.sort((a, b) => nameLower(b).localeCompare(nameLower(a)));
      case 'recent':
        // installed_at is RFC 3339; string compare works for ISO dates,
        // newest first.
        return xs.sort((a, b) => b.installed.installed_at.localeCompare(a.installed.installed_at));
      case 'source':
        // Modrinth / CurseForge / manual (null), then alphabetic
        // within each group.
        return xs.sort((a, b) => {
          const sa = a.installed.source ?? 'zz-manual';
          const sb = b.installed.source ?? 'zz-manual';
          if (sa !== sb) return sa.localeCompare(sb);
          return nameLower(a).localeCompare(nameLower(b));
        });
    }
  });

  const filtered = $derived(
    sorted
      .filter((r) => {
        if (enabledFilter === 'enabled') return r.installed.enabled;
        if (enabledFilter === 'disabled') return !r.installed.enabled;
        return true;
      })
      .filter(
        (r) =>
          filter.trim() === '' || rowDisplayName(r).toLowerCase().includes(filter.toLowerCase()),
      ),
  );

  // Counters reflect the full installed list (pre-filter) so the user
  // sees the inventory total even when narrowing by name.
  const totalCount = $derived(rows.length);
  const enabledCount = $derived(rows.filter((r) => r.installed.enabled).length);
  const disabledCount = $derived(totalCount - enabledCount);

  async function refresh() {
    if (!instanceId) {
      rows = [];
      return;
    }
    loading = true;
    error = null;
    let r = await commands.modsListInstalled(instanceId);
    if (r.status === 'error') {
      error = formatError(r.error);
      loading = false;
      return;
    }

    // Pack-origin chip data — a local file read, no network.
    const ps = await commands.modsPackOriginSummary(instanceId);
    const summary = ps.status === 'ok' ? ps.data : null;
    packSummary = summary;

    // Backfill: if this instance has modpack override-bundled mods that
    // still lack a platform identity and have never been hash-enriched,
    // run one enrichment pass and re-fetch the list. `enrich_attempted`
    // flips true for every mod the pass tries, so this branch can never
    // run twice for the same mod — no loop.
    if (
      summary &&
      r.data.some(
        (m) =>
          m.source === null &&
          !m.enrich_attempted &&
          summary.mod_shas.includes(m.sha1),
      )
    ) {
      await commands.modsEnrichPackMods(instanceId);
      const r2 = await commands.modsListInstalled(instanceId);
      if (r2.status === 'ok') r = r2;
    }

    // Fetch ModSummary for every platform-installed mod in parallel.
    // Manual / unidentifiable mods (source: null) skip the fetch and
    // stay as a degraded row. If a project lookup fails (network blip,
    // mod taken down upstream), the row still renders with the
    // locally-cached name.
    const enriched = await Promise.all(
      r.data.map(async (m): Promise<Row> => {
        if (m.source === null || m.project_id === null) {
          return { summary: null, installed: m };
        }
        const p = await commands.modsProject(m.source as ModSource, m.project_id);
        if (p.status === 'ok') {
          return { summary: p.data.summary, installed: m };
        }
        return { summary: null, installed: m };
      }),
    );
    rows = enriched;
    loading = false;
  }

  $effect(() => {
    // biome-ignore lint/correctness/noUnusedVariables: reactive read on instanceId
    const _id = instanceId;
    void refresh();
  });

  let unlisteners: Array<() => void> = [];

  onMount(async () => {
    const handlers = [
      events.modInstalled.listen(() => void refresh()),
      events.modUninstalled.listen(() => void refresh()),
      events.modToggle.listen(() => void refresh()),
    ];
    for (const p of handlers) {
      unlisteners.push(await p);
    }
  });

  onDestroy(() => {
    for (const u of unlisteners) u();
    unlisteners = [];
  });

  async function toggle(m: InstalledMod) {
    if (!instanceId) return;
    busy = true;
    error = null;
    const result = m.enabled
      ? await commands.modsDisable(instanceId, m.sha1)
      : await commands.modsEnable(instanceId, m.sha1);
    if (result.status === 'error') {
      error = formatError(result.error);
    } else {
      await refresh();
    }
    busy = false;
  }

  async function uninstall(m: InstalledMod) {
    if (!instanceId) return;
    busy = true;
    error = null;
    const result = await commands.modsUninstall(instanceId, m.sha1);
    if (result.status === 'error') {
      error = formatError(result.error);
    } else {
      await refresh();
    }
    busy = false;
  }

  async function switchVersion(row: Row, v: ModVersion) {
    if (!instanceId) return;
    drawerRow = null;
    busy = true;
    error = null;
    // Uninstall the old version, then install the picked one. If the
    // install half fails after the uninstall succeeded the user is left
    // with no mod — surface the error so they can retry.
    const removed = await commands.modsUninstall(instanceId, row.installed.sha1);
    if (removed.status === 'error') {
      error = formatError(removed.error);
      busy = false;
      return;
    }
    const installed = await commands.modsInstallWithDeps(
      instanceId,
      { source: v.source, project_id: v.project_id, version_id: v.version_id },
      [],
    );
    if (installed.status === 'error') {
      pushWarning('Mod install failed', [formatError(installed.error)]);
    } else {
      pushSuccess(`Installed ${rowDisplayName(row)}`);
    }
    busy = false;
    await refresh();
  }

  // Seed the update map from the session cache when the instance changes.
  $effect(() => {
    const id = instanceId;
    if (id) {
      const cached = updateCheckCache.get(id);
      updateChecks = cached ? new Map(cached.map((c) => [c.sha1, c])) : new Map();
    } else {
      updateChecks = new Map();
    }
  });

  async function checkUpdates() {
    if (!instanceId) return;
    checking = true;
    error = null;
    const r = await commands.modsCheckUpdates(instanceId);
    checking = false;
    if (r.status === 'error') {
      error = formatError(r.error);
      showCfBanner = false;
      return;
    }
    updateChecks = new Map(r.data.map((c) => [c.sha1, c]));
    updateCheckCache.set(instanceId, r.data);
    // If a CurseForge mod's check failed, it may be a missing API key —
    // surface the banner only when the key really is absent.
    const cfFailed = r.data.some(
      (c) => c.source === 'curseforge' && c.state.kind === 'check_failed',
    );
    if (cfFailed) {
      const s = await commands.modsGetCurseforgeKeyStatus();
      showCfBanner = s.status === 'ok' && s.data === 'missing';
    } else {
      showCfBanner = false;
    }
  }

  async function applyUpdate(sha1: string, target: ModVersion): Promise<boolean> {
    if (!instanceId) return false;
    const r = await commands.modsUpdateOne(instanceId, sha1, target);
    if (r.status === 'error') {
      error = formatError(r.error);
      return false;
    }
    return true;
  }

  async function updateOne(m: InstalledMod) {
    const check = updateChecks.get(m.sha1);
    if (!instanceId || !check || check.state.kind !== 'update_available') return;
    busy = true;
    error = null;
    const ok = await applyUpdate(m.sha1, check.state.target);
    if (ok) {
      // The stale check no longer applies — drop it; the row's version
      // is now current. The user can re-check to refresh the rest.
      const next = new Map(updateChecks);
      next.delete(m.sha1);
      updateChecks = next;
      updateCheckCache.set(instanceId, [...next.values()]);
    }
    busy = false;
    await refresh();
  }

  async function updateAll() {
    if (!instanceId) return;
    const targets = [...updateChecks.values()].flatMap((c) =>
      c.state.kind === 'update_available' ? [{ sha1: c.sha1, target: c.state.target }] : [],
    );
    if (targets.length === 0) return;
    busy = true;
    error = null;
    let ok = 0;
    let failed = 0;
    for (const t of targets) {
      if (await applyUpdate(t.sha1, t.target)) ok++;
      else failed++;
    }
    // Every check is now stale (versions moved) — clear and let the user
    // re-check.
    updateChecks = new Map();
    updateCheckCache.delete(instanceId);
    busy = false;
    await refresh();
    if (failed === 0) {
      pushSuccess(`Updated ${ok} mod${ok === 1 ? '' : 's'}`);
    } else {
      pushWarning(`Updated ${ok}, ${failed} failed`, []);
    }
  }
</script>

<div class="p-3">
  <div class="mb-2 space-y-2">
    {#if totalCount > 0}
      <div class="text-xs text-neutral-500 flex gap-3">
        <span>Total: <span class="font-medium text-neutral-700">{totalCount}</span></span>
        <span>Enabled: <span class="font-medium text-green-700">{enabledCount}</span></span>
        <span>Disabled: <span class="font-medium text-neutral-700">{disabledCount}</span></span>
      </div>
    {/if}
    <div class="flex gap-2 items-center">
      <input
        type="search"
        placeholder="Filter installed…"
        aria-label="Filter installed mods"
        class="flex-1 border border-neutral-300 rounded px-3 py-1.5 text-sm"
        bind:value={filter}
      />
      <label class="text-xs text-neutral-600 inline-flex items-center gap-1">
        Sort:
        <select bind:value={sortBy} class="border rounded px-2 py-1 text-xs bg-white">
          <option value="name-asc">Name (A → Z)</option>
          <option value="name-desc">Name (Z → A)</option>
          <option value="recent">Recently installed</option>
          <option value="source">Source</option>
        </select>
      </label>
      <button
        type="button"
        class="text-xs px-2 py-1 border rounded bg-white hover:bg-neutral-50 disabled:opacity-50"
        disabled={busy || checking || rows.length === 0}
        onclick={checkUpdates}
      >
        {checking ? 'Checking…' : 'Check for updates'}
      </button>
      {#if updateCount > 0}
        <button
          type="button"
          class="text-xs px-2 py-1 border border-amber-300 rounded bg-amber-100 text-amber-900 hover:bg-amber-200 disabled:opacity-50"
          disabled={busy}
          onclick={updateAll}
        >
          Update all ({updateCount})
        </button>
      {/if}
    </div>
    {#if totalCount > 0}
      <div role="tablist" aria-label="Filter by state" class="flex gap-1 text-xs">
        <button
          type="button"
          role="tab"
          aria-selected={enabledFilter === 'all'}
          class="px-2 py-1 rounded border"
          class:bg-blue-50={enabledFilter === 'all'}
          class:text-blue-700={enabledFilter === 'all'}
          class:font-medium={enabledFilter === 'all'}
          class:bg-white={enabledFilter !== 'all'}
          onclick={() => (enabledFilter = 'all')}
        >
          All ({totalCount})
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={enabledFilter === 'enabled'}
          class="px-2 py-1 rounded border"
          class:bg-green-50={enabledFilter === 'enabled'}
          class:text-green-700={enabledFilter === 'enabled'}
          class:font-medium={enabledFilter === 'enabled'}
          class:bg-white={enabledFilter !== 'enabled'}
          onclick={() => (enabledFilter = 'enabled')}
        >
          Enabled ({enabledCount})
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={enabledFilter === 'disabled'}
          class="px-2 py-1 rounded border"
          class:bg-neutral-100={enabledFilter === 'disabled'}
          class:text-neutral-700={enabledFilter === 'disabled'}
          class:font-medium={enabledFilter === 'disabled'}
          class:bg-white={enabledFilter !== 'disabled'}
          onclick={() => (enabledFilter = 'disabled')}
        >
          Disabled ({disabledCount})
        </button>
      </div>
    {/if}
  </div>

  {#if error}
    <div class="bg-red-50 border border-red-200 text-red-900 text-sm rounded p-2 mb-2">
      {error}
    </div>
  {/if}
  {#if showCfBanner}
    <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'curseforge' })} />
  {/if}

  {#if !instanceId}
    <div class="text-neutral-400 text-sm py-8 text-center">Pick an instance first.</div>
  {:else if loading && rows.length === 0}
    <div class="text-neutral-400 text-sm py-8 text-center">Loading installed mods…</div>
  {:else if rows.length === 0}
    <div class="text-neutral-400 text-sm py-8 text-center">
      No mods installed in this instance yet.
    </div>
  {:else}
    <div class="space-y-2">
      {#each filtered as row (row.installed.sha1)}
        {#if row.summary}
          <ModCard
            summary={row.summary}
            installed={row.installed}
            onInstall={() => {}}
            onOpenDetail={() => (drawerRow = row)}
            onToggle={() => toggle(row.installed)}
            onUninstall={() => uninstall(row.installed)}
            updateState={updateChecks.get(row.installed.sha1)?.state ?? null}
            onUpdate={() => updateOne(row.installed)}
            {checking}
            packChip={packSummary && packSummary.mod_shas.includes(row.installed.sha1)
              ? packSummary.project_name
              : null}
          />
        {:else}
          <!-- No platform metadata. Either a hand-dropped "manual mod"
               or a modpack override-bundled jar that hash-enrichment
               could not identify ("from modpack" + 📦 chip). -->
          {@const fromPack =
            !!packSummary && packSummary.mod_shas.includes(row.installed.sha1)}
          <div class="border border-neutral-200 rounded bg-white p-3 flex gap-3">
            <div
              class="w-12 h-12 rounded bg-neutral-100 flex items-center justify-center text-neutral-400"
              aria-hidden="true"
            >
              ◆
            </div>
            <div class="flex-1 min-w-0">
              <div class="font-medium text-neutral-900 truncate">{row.installed.filename}</div>
              <div class="text-xs text-neutral-500 truncate">
                {fromPack ? 'from modpack' : 'manual mod'} · {row.installed.enabled
                  ? 'Enabled'
                  : 'Disabled'}
              </div>
            </div>
            <div class="self-center flex items-center gap-1">
              {#if fromPack && packSummary}
                <span
                  class="text-xs px-2 py-1 rounded bg-indigo-50 text-indigo-700"
                  title="From modpack: {packSummary.project_name}"
                >
                  📦 {packSummary.project_name}
                </span>
              {/if}
              <button
                type="button"
                class="text-xs px-2 py-1 border rounded"
                disabled={busy}
                onclick={() => toggle(row.installed)}
              >
                {row.installed.enabled ? 'Disable' : 'Enable'}
              </button>
              <button
                type="button"
                class="text-xs px-2 py-1 border rounded text-red-700 hover:bg-red-50"
                disabled={busy}
                onclick={() => uninstall(row.installed)}
              >
                Uninstall
              </button>
            </div>
          </div>
        {/if}
      {/each}
    </div>
  {/if}

  {#if drawerRow && drawerRow.installed.source && drawerRow.installed.project_id && instanceId}
    <ModDetailDrawer
      source={drawerRow.installed.source as ModSource}
      projectId={drawerRow.installed.project_id}
      {mcVersion}
      {loader}
      installedVersionId={drawerRow.installed.version_id}
      onClose={() => (drawerRow = null)}
      onInstall={(v) => {
        if (drawerRow) void switchVersion(drawerRow, v);
      }}
    />
  {/if}
</div>

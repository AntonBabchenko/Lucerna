<script lang="ts">
  import {
    commands,
    events,
    type DepRoot,
    type DepTreeNode,
    type DependencyGraph,
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
  import DepTree from './DepTree.svelte';
  import ModCard from './ModCard.svelte';
  import ModDetailModal from './ModDetailModal.svelte';
  import OrphanUninstallDialog from './OrphanUninstallDialog.svelte';
  import { depGraphCache } from './dep-graph-cache';
  import { updateCheckCache } from './update-check-cache';
  import type { OrphanRef } from '$lib/ipc/bindings';

  // The Installed pane of ModBrowserTab. Renders the same ModCard
  // component the Browse pane uses, so the UI is consistent — same
  // icons, layout, Disable/Enable + Uninstall affordances. The only
  // difference is the list is filtered to mods currently installed in
  // the active instance.
  //
  // Each row pairs a ModSummary (fetched lazily from the platform per
  // installed mod's project_id) with the InstalledMod record from
  // {instance}/lucerna/installed-mods.json. Manual mods (jars the
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

  // WCAG radiogroup keyboard pattern. Arrow / Home / End moves selection
  // within the group; the newly-checked radio gets focus so a screen reader
  // announces the change. Roving tabindex (0 on checked, -1 elsewhere) keeps
  // the whole group as one tab stop.
  const FILTER_VALUES = ['all', 'enabled', 'disabled'] as const;
  function handleFilterKey(e: KeyboardEvent) {
    const i = FILTER_VALUES.indexOf(enabledFilter);
    let next: (typeof FILTER_VALUES)[number] | null = null;
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
      next = FILTER_VALUES[(i + 1) % FILTER_VALUES.length];
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
      next = FILTER_VALUES[(i - 1 + FILTER_VALUES.length) % FILTER_VALUES.length];
    } else if (e.key === 'Home') {
      next = FILTER_VALUES[0];
    } else if (e.key === 'End') {
      next = FILTER_VALUES[FILTER_VALUES.length - 1];
    }
    if (next !== null) {
      e.preventDefault();
      enabledFilter = next;
      const target = e.currentTarget as HTMLElement | null;
      const btn = target?.querySelector<HTMLButtonElement>(`button[data-value="${next}"]`);
      btn?.focus();
    }
  }
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

  // Dependency graph — loaded in the background, never blocks the mod list.
  let graph = $state<DependencyGraph | null>(null);
  let graphLoading = $state(false);
  // Set of installed-mod sha1s whose dep subtree is currently expanded.
  // Reassign the whole Set (never mutate in place) to trigger reactivity.
  let expanded = $state<Set<string>>(new Set());
  // Hovered dep-key (`source:project_id`) for cross-highlight. Shared
  // between row wrappers and DepTree nodes so every on-screen occurrence
  // of the same mod lights up together.
  let hoveredKey = $state<string | null>(null);

  // Returns the canonical highlight key for a mod. Platform mods use
  // `source:project_id`; manual mods (source or project_id absent) fall
  // back to `sha1:<sha1>` so they only highlight themselves.
  const modKey = (source: string | null, projectId: string | null, sha1: string) =>
    source && projectId ? `${source}:${projectId}` : `sha1:${sha1}`;

  // Strategy C: render the mod list immediately; resolve the graph in the
  // background. Seed from the session cache on instance change so switching
  // back to an already-visited instance is instant.
  $effect(() => {
    const id = instanceId;
    graph = id ? (depGraphCache.get(id) ?? null) : null;
    if (id) void loadGraph(id);
  });

  async function loadGraph(id: string) {
    graphLoading = true;
    const r = await commands.modsDependencyGraph(id);
    graphLoading = false;
    if (r.status === 'ok') {
      graph = r.data;
      depGraphCache.set(id, r.data);
    }
  }

  function recheckDeps() {
    if (instanceId) {
      depGraphCache.delete(instanceId);
      void loadGraph(instanceId);
    }
  }

  const rootBySha = $derived(new Map((graph?.roots ?? []).map((r) => [r.sha1, r])));

  // Build a reverse map: for each installed mod's project_id, list the
  // display names of roots that depend on it (via required subtree).
  const requiredBy = $derived.by(() => {
    const map = new Map<string, string[]>();
    for (const r of graph?.roots ?? []) {
      const seen = new Set<string>();
      const walk = (ns: DepTreeNode[]) => {
        for (const n of ns) {
          if (n.status === 'satisfied' && !seen.has(n.project_id)) {
            seen.add(n.project_id);
            map.set(n.project_id, [...(map.get(n.project_id) ?? []), r.name]);
          }
          if (!n.cycle) walk(n.children);
        }
      };
      walk(r.required);
    }
    return map;
  });

  function depCounts(root: DepRoot | undefined) {
    if (!root) return { total: 0, missing: 0 };
    let total = 0;
    let missing = 0;
    const walk = (ns: DepTreeNode[]) => {
      for (const n of ns) {
        total++;
        if (n.status === 'missing_required') missing++;
        if (!n.cycle) walk(n.children);
      }
    };
    walk(root.required);
    return { total, missing };
  }

  function toggleExpand(sha1: string) {
    const next = new Set(expanded);
    if (next.has(sha1)) next.delete(sha1);
    else next.add(sha1);
    expanded = next;
  }

  async function installDepNode(node: DepTreeNode) {
    if (!instanceId || !mcVersion || !loader) return;
    busy = true;
    error = null;
    const vr = await commands.modsVersions(node.source, node.project_id, mcVersion, loader);
    if (vr.status === 'error' || vr.data.length === 0) {
      error =
        vr.status === 'error' ? formatError(vr.error) : `No compatible version of ${node.name}`;
      busy = false;
      return;
    }
    const primary = vr.data[0];
    const res = await commands.modsInstallWithDeps(
      instanceId,
      { source: primary.source, project_id: primary.project_id, version_id: primary.version_id },
      [],
    );
    if (res.status === 'error') {
      pushWarning('Install failed', [formatError(res.error)]);
    } else {
      pushSuccess(`Installed ${node.name}`);
    }
    busy = false;
    if (instanceId) {
      depGraphCache.delete(instanceId);
      await loadGraph(instanceId);
    }
    await refresh();
  }

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

  // Selection state for bulk actions. `selected` holds the sha1s of the
  // currently checked rows. Reassign the whole Set (never mutate in place)
  // to trigger Svelte 5 reactivity.
  let selected = $state<Set<string>>(new Set());

  // Drop selections for rows no longer visible (filter/search change) so a
  // hidden row can't be bulk-acted on.
  $effect(() => {
    const visible = new Set(filtered.map((r) => r.installed.sha1));
    let changed = false;
    const next = new Set(selected);
    for (const sha of next)
      if (!visible.has(sha)) {
        next.delete(sha);
        changed = true;
      }
    if (changed) selected = next;
  });
  $effect(() => {
    // biome-ignore lint/correctness/noUnusedVariables: reactive read on instanceId
    const _id = instanceId;
    selected = new Set();
  });

  const allSelected = $derived(
    filtered.length > 0 && filtered.every((r) => selected.has(r.installed.sha1)),
  );
  function toggleSelect(sha1: string, checked: boolean) {
    const next = new Set(selected);
    if (checked) next.add(sha1);
    else next.delete(sha1);
    selected = next;
  }
  function toggleSelectAll(checked: boolean) {
    selected = checked ? new Set(filtered.map((r) => r.installed.sha1)) : new Set();
  }

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
    // flips true only when EVERY platform the pass tried responded
    // successfully — a transient CF/Modrinth outage leaves the flag
    // unflipped, so this branch can fire again on a later Installed-tab
    // open until both platforms answered. Bounded by tab events, not a
    // hot loop.
    if (
      summary &&
      r.data.some(
        (m) => m.source === null && !m.enrich_attempted && summary.mod_shas.includes(m.sha1),
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
      // `installed_dependencies` carries release titles, not mod names — omit
      // them here; the clean reinstall title is the meaningful signal.
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

  const selectedRows = $derived(filtered.filter((r) => selected.has(r.installed.sha1)));
  const selectedUpdatable = $derived(
    selectedRows.filter(
      (r) => updateChecks.get(r.installed.sha1)?.state.kind === 'update_available',
    ),
  );

  async function bulkSetEnabled(enable: boolean) {
    if (!instanceId || selected.size === 0) return;
    busy = true;
    error = null;
    let ok = 0,
      failed = 0;
    for (const r of selectedRows) {
      if (r.installed.enabled === enable) {
        ok++;
        continue;
      }
      const res = enable
        ? await commands.modsEnable(instanceId, r.installed.sha1)
        : await commands.modsDisable(instanceId, r.installed.sha1);
      if (res.status === 'error') failed++;
      else ok++;
    }
    busy = false;
    selected = new Set();
    await refresh();
    if (failed === 0)
      pushSuccess(`${enable ? 'Enabled' : 'Disabled'} ${ok} mod${ok === 1 ? '' : 's'}`);
    else pushWarning(`${enable ? 'Enabled' : 'Disabled'} ${ok}, ${failed} failed`, []);
  }

  async function bulkUpdate() {
    if (!instanceId) return;
    const targets = selectedUpdatable.flatMap((r) => {
      const st = updateChecks.get(r.installed.sha1)?.state;
      return st?.kind === 'update_available' ? [{ sha1: r.installed.sha1, target: st.target }] : [];
    });
    if (targets.length === 0) return;
    busy = true;
    error = null;
    let ok = 0,
      failed = 0;
    for (const t of targets) {
      if (await applyUpdate(t.sha1, t.target)) ok++;
      else failed++;
    }
    updateChecks = new Map();
    updateCheckCache.delete(instanceId);
    busy = false;
    selected = new Set();
    await refresh();
    if (failed === 0) pushSuccess(`Updated ${ok} mod${ok === 1 ? '' : 's'}`);
    else pushWarning(`Updated ${ok}, ${failed} failed`, []);
  }

  let uninstallPrompt = $state<{
    removing: string[];
    names: string[];
    orphans: OrphanRef[];
  } | null>(null);

  async function requestBulkUninstall() {
    if (!instanceId || selected.size === 0) return;
    const removing = selectedRows.map((r) => r.installed.sha1);
    const names = selectedRows.map((r) => rowDisplayName(r));
    const r = await commands.modsFindOrphans(instanceId, removing);
    const orphans = r.status === 'ok' ? r.data : [];
    uninstallPrompt = { removing, names, orphans };
  }

  async function confirmBulkUninstall(alsoRemove: string[]) {
    if (!instanceId || !uninstallPrompt) return;
    const all = [...uninstallPrompt.removing, ...alsoRemove];
    uninstallPrompt = null;
    busy = true;
    error = null;
    let ok = 0,
      failed = 0;
    for (const sha1 of all) {
      const res = await commands.modsUninstall(instanceId, sha1);
      if (res.status === 'error') failed++;
      else ok++;
    }
    busy = false;
    selected = new Set();
    await refresh();
    if (failed === 0) pushSuccess(`Uninstalled ${ok} mod${ok === 1 ? '' : 's'}`);
    else pushWarning(`Uninstalled ${ok}, ${failed} failed`, []);
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
      <div class="text-xs text-muted flex gap-3">
        <span>Total: <span class="font-medium text-secondary">{totalCount}</span></span>
        <span>Enabled: <span class="font-medium text-success">{enabledCount}</span></span>
        <span>Disabled: <span class="font-medium text-secondary">{disabledCount}</span></span>
      </div>
    {/if}
    <div class="flex flex-wrap gap-2 items-center">
      <input
        type="search"
        placeholder="Filter installed…"
        aria-label="Filter installed mods"
        class="flex-1 border border-border-emphasis rounded px-3 py-1.5 text-sm"
        bind:value={filter}
      />
      <label class="text-xs text-secondary inline-flex items-center gap-1">
        Sort:
        <select bind:value={sortBy} class="border rounded px-2 py-1 text-xs bg-surface">
          <option value="name-asc">Name (A → Z)</option>
          <option value="name-desc">Name (Z → A)</option>
          <option value="recent">Recently installed</option>
          <option value="source">Source</option>
        </select>
      </label>
      <label class="text-xs text-secondary inline-flex items-center gap-1">
        <input
          type="checkbox"
          aria-label="Select all"
          checked={allSelected}
          onchange={(e) => toggleSelectAll((e.currentTarget as HTMLInputElement).checked)}
        />
        Select all
      </label>
      <button
        type="button"
        class="btn-secondary btn-xs"
        disabled={busy || checking || rows.length === 0}
        onclick={checkUpdates}
      >
        {checking ? 'Checking…' : 'Check for updates'}
      </button>
      <button
        type="button"
        class="btn-secondary btn-xs"
        disabled={graphLoading}
        onclick={recheckDeps}
      >
        {graphLoading ? 'Resolving…' : '↻ Re-check deps'}
      </button>
      {#if updateCount > 0}
        <button type="button" class="btn-warning btn-xs" disabled={busy} onclick={updateAll}>
          Update all ({updateCount})
        </button>
      {/if}
    </div>
    {#if totalCount > 0}
      <div
        role="radiogroup"
        aria-label="Mod filter"
        tabindex={-1}
        class="flex gap-1 text-xs"
        onkeydown={handleFilterKey}
      >
        <button
          type="button"
          role="radio"
          aria-checked={enabledFilter === 'all'}
          tabindex={enabledFilter === 'all' ? 0 : -1}
          data-value="all"
          class="btn-secondary btn-xs"
          class:bg-accent-soft={enabledFilter === 'all'}
          class:text-accent={enabledFilter === 'all'}
          class:font-medium={enabledFilter === 'all'}
          onclick={() => (enabledFilter = 'all')}
        >
          All ({totalCount})
        </button>
        <button
          type="button"
          role="radio"
          aria-checked={enabledFilter === 'enabled'}
          tabindex={enabledFilter === 'enabled' ? 0 : -1}
          data-value="enabled"
          class="btn-secondary btn-xs"
          class:bg-success-bg={enabledFilter === 'enabled'}
          class:text-success={enabledFilter === 'enabled'}
          class:font-medium={enabledFilter === 'enabled'}
          onclick={() => (enabledFilter = 'enabled')}
        >
          Enabled ({enabledCount})
        </button>
        <button
          type="button"
          role="radio"
          aria-checked={enabledFilter === 'disabled'}
          tabindex={enabledFilter === 'disabled' ? 0 : -1}
          data-value="disabled"
          class="btn-secondary btn-xs"
          class:bg-subtle={enabledFilter === 'disabled'}
          class:text-secondary={enabledFilter === 'disabled'}
          class:font-medium={enabledFilter === 'disabled'}
          onclick={() => (enabledFilter = 'disabled')}
        >
          Disabled ({disabledCount})
        </button>
      </div>
    {/if}
  </div>

  {#if error}
    <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-2">
      {error}
    </div>
  {/if}
  {#if showCfBanner}
    <CurseForgeKeyBanner onOpenSettings={() => (settingsOpen.value = { tab: 'curseforge' })} />
  {/if}

  {#if !instanceId}
    <div class="text-placeholder text-sm py-8 text-center">Pick an instance first.</div>
  {:else if loading && rows.length === 0}
    <div class="text-placeholder text-sm py-8 text-center">Loading installed mods…</div>
  {:else if rows.length === 0}
    <div class="text-placeholder text-sm py-8 text-center">
      No mods installed in this instance yet.
    </div>
  {:else}
    {#if selected.size > 0}
      <div
        data-testid="bulk-bar"
        class="sticky top-0 z-10 flex items-center gap-2 bg-accent-soft border border-accent rounded px-3 py-2 mb-2 text-sm"
      >
        <span class="font-medium text-accent">{selected.size} selected</span>
        <div class="ml-auto flex items-center gap-1">
          <button
            type="button"
            class="btn-secondary btn-xs"
            disabled={busy}
            onclick={() => bulkSetEnabled(true)}>Enable</button
          >
          <button
            type="button"
            class="btn-secondary btn-xs"
            disabled={busy}
            onclick={() => bulkSetEnabled(false)}>Disable</button
          >
          <button
            type="button"
            class="btn-secondary btn-xs"
            disabled={busy || selectedUpdatable.length === 0}
            title={selectedUpdatable.length === 0
              ? 'Run "Check for updates" first; only mods with a pending update can be updated'
              : ''}
            onclick={bulkUpdate}>Update</button
          >
          <button
            type="button"
            class="btn-ghost-danger btn-xs"
            disabled={busy}
            onclick={requestBulkUninstall}>Uninstall</button
          >
          <button type="button" class="btn-ghost btn-xs" onclick={() => (selected = new Set())}
            >Clear</button
          >
        </div>
      </div>
    {/if}
    <div class="border border-border-subtle rounded overflow-hidden">
      {#each filtered as row (row.installed.sha1)}
        {#if row.summary}
          {@const rowKey = modKey(
            row.installed.source,
            row.installed.project_id,
            row.installed.sha1,
          )}
          {@const root = rootBySha.get(row.installed.sha1)}
          {@const counts = depCounts(root)}
          {@const reqBy = requiredBy.get(row.installed.project_id ?? '') ?? []}
          <div
            data-mod-key={rowKey}
            class:bg-highlight={hoveredKey === rowKey}
            onmouseenter={() => (hoveredKey = rowKey)}
            onmouseleave={() => (hoveredKey = null)}
            role="group"
          >
            <ModCard
              layout="list"
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
              selectable={true}
              selected={selected.has(row.installed.sha1)}
              onSelectChange={(c) => toggleSelect(row.installed.sha1, c)}
            />
            <div class="flex items-center gap-2 px-3 pb-1 text-xs">
              {#if graphLoading && !root}
                <span class="text-placeholder">resolving…</span>
              {:else}
                {#if counts.total > 0}
                  <button
                    type="button"
                    class="px-2 py-0.5 rounded bg-accent-soft text-accent"
                    onclick={() => toggleExpand(row.installed.sha1)}
                  >
                    {expanded.has(row.installed.sha1) ? '▾' : '▸'}
                    {counts.total} dep{counts.total === 1 ? '' : 's'}{counts.missing > 0
                      ? ` · ${counts.missing} missing`
                      : ''}
                  </button>
                {/if}
                {#if reqBy.length > 0}
                  <button
                    type="button"
                    class="px-2 py-0.5 rounded bg-subtle text-secondary"
                    onclick={() => toggleExpand(row.installed.sha1)}
                    >required by {reqBy.length}</button
                  >
                {/if}
              {/if}
            </div>
            {#if expanded.has(row.installed.sha1) && root}
              <div class="px-4 pb-3 bg-subtle/40">
                {#if root.required.length > 0}
                  <div class="text-[10px] uppercase tracking-wide text-muted mt-1">Requires</div>
                  <DepTree
                    nodes={root.required}
                    {hoveredKey}
                    onHover={(k) => (hoveredKey = k)}
                    onInstall={installDepNode}
                    onAdd={installDepNode}
                  />
                {/if}
                {#if root.optional.length > 0}
                  <div class="text-[10px] uppercase tracking-wide text-muted mt-2">
                    Recommended · optional
                  </div>
                  <DepTree
                    nodes={root.optional}
                    {hoveredKey}
                    onHover={(k) => (hoveredKey = k)}
                    onInstall={installDepNode}
                    onAdd={installDepNode}
                  />
                {/if}
                {#if reqBy.length > 0}
                  <div class="text-[10px] uppercase tracking-wide text-muted mt-2">Required by</div>
                  <div class="text-xs text-secondary">{reqBy.join(', ')}</div>
                {/if}
              </div>
            {/if}
          </div>
        {:else}
          <!-- No platform metadata. Either a hand-dropped "manual mod"
               or a modpack override-bundled jar that hash-enrichment
               could not identify ("from modpack" + 📦 chip). -->
          {@const fromPack = !!packSummary && packSummary.mod_shas.includes(row.installed.sha1)}
          {@const mKey = modKey(row.installed.source, row.installed.project_id, row.installed.sha1)}
          <div
            class="flex items-center gap-3 px-3 py-2 border-b border-border-subtle bg-surface"
            data-testid="manual-mod-row"
            data-mod-key={mKey}
            class:bg-highlight={hoveredKey === mKey}
            onmouseenter={() => (hoveredKey = mKey)}
            onmouseleave={() => (hoveredKey = null)}
            role="group"
          >
            <input
              type="checkbox"
              class="flex-shrink-0"
              checked={selected.has(row.installed.sha1)}
              aria-label={`Select mod ${row.installed.filename}`}
              onchange={(e) =>
                toggleSelect(row.installed.sha1, (e.currentTarget as HTMLInputElement).checked)}
            />
            <div
              class="w-8 h-8 rounded bg-subtle flex items-center justify-center text-placeholder text-xs flex-shrink-0"
              aria-hidden="true"
            >
              ◆
            </div>
            <div class="flex-1 min-w-0">
              <div class="font-medium text-primary truncate">{row.installed.filename}</div>
              <div class="text-xs text-muted truncate">
                {fromPack ? 'from modpack' : 'manual mod'} · {row.installed.enabled
                  ? 'Enabled'
                  : 'Disabled'}
              </div>
            </div>
            <div class="flex items-center gap-1 flex-shrink-0">
              {#if fromPack && packSummary}
                <span
                  class="text-xs px-2 py-0.5 rounded bg-accent-soft text-accent"
                  title="From modpack: {packSummary.project_name}"
                  >📦 {packSummary.project_name}</span
                >
              {/if}
              <button
                type="button"
                class="btn-secondary btn-xs"
                disabled={busy}
                onclick={() => toggle(row.installed)}
                >{row.installed.enabled ? 'Disable' : 'Enable'}</button
              >
              <button
                type="button"
                class="btn-ghost-danger btn-xs"
                disabled={busy}
                onclick={() => uninstall(row.installed)}>Uninstall</button
              >
            </div>
          </div>
        {/if}
      {/each}
    </div>
  {/if}

  {#if drawerRow && drawerRow.installed.source && drawerRow.installed.project_id && instanceId}
    <ModDetailModal
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

  {#if uninstallPrompt}
    <OrphanUninstallDialog
      removingNames={uninstallPrompt.names}
      orphans={uninstallPrompt.orphans}
      onCancel={() => (uninstallPrompt = null)}
      onConfirm={confirmBulkUninstall}
    />
  {/if}
</div>

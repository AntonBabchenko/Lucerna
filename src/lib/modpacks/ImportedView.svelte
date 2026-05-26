<script lang="ts">
  import { commands } from '$lib/ipc/bindings';
  import type { InstanceWithStatus, LoaderKind, ModpackStatus } from '$lib/ipc/bindings';
  import { modpacksNav } from '$lib/settings/state.svelte';
  import ImportedCard from './ImportedCard.svelte';
  import ImportedDetailDrawer from './ImportedDetailDrawer.svelte';

  // Filtered grid of instances that originated from a modpack import
  // (i.e. `mrpack_name` is set on the metadata). The toolbar above the
  // grid lets the user narrow by text (matches pack name OR instance
  // name), MC version, loader, and sort order. All filtering is
  // client-side — the imported list is tiny (single-digit to maybe a
  // few dozen for power users), so there's no point pushing this onto
  // an IPC round-trip. Clicking a card opens the detail drawer; the
  // drawer's "Open instance" button is what fires `onPick` back to the
  // parent (= switch active instance).
  //
  // We compute `mcOptions` from the set of MC versions actually present
  // across imported packs, sorted descending so newer versions surface
  // first. Loader options are the static enum — easier to read than
  // deriving them and gives a stable shape regardless of which packs
  // happen to be imported right now.
  //
  // Empty-state copy distinguishes "user hasn't imported anything yet"
  // (no packs at all) from "filter dropped the visible set to zero"
  // (packs exist but none match the current toolbar values).
  //
  // `onListChanged` propagates a list-shape-changed signal from a delete
  // inside the drawer up to the parent (ModpacksTab → MainTabs →
  // +page.svelte's `refreshInstances`). Without it the card stays in
  // the grid after a successful delete until the next external refresh.

  let {
    instances,
    onPick,
    onListChanged,
  }: {
    instances: InstanceWithStatus[];
    onPick: (id: string) => void;
    onListChanged?: () => void;
  } = $props();

  let drawerInstId = $state<string | null>(null);
  // Derive the drawer's instance from the live `instances` prop (not a
  // snapshot) so a post-update / post-restore refresh of the list
  // flows into the open drawer's header.
  const drawerInst = $derived(
    drawerInstId == null ? null : (instances.find((i) => i.id === drawerInstId) ?? null),
  );

  // Final consumer of modpacksNav: open the drawer for the requested
  // instance and clear the rune so later in-tab clicks aren't hijacked.
  $effect(() => {
    const nav = modpacksNav.value;
    if (nav !== null) {
      drawerInstId = nav.openDrawerForInstance;
      modpacksNav.value = null;
    }
  });

  let query = $state('');
  let mcFilter = $state('');
  let loaderFilter = $state<'' | LoaderKind>('');
  let sort = $state<'newest' | 'oldest' | 'name'>('newest');

  // Status cache (bundle 2): one entry per pack-instance, keyed by
  // instance id. Populated on mount and whenever `instances` changes
  // shape (new pack imported, instance deleted). `null` means "we
  // tried but the instance has no pack_origin" (manually created, or
  // pre-bundle-2 import). `undefined` means "haven't fetched yet";
  // ImportedCard treats both as "not modified". We re-fetch on every
  // `instances` change rather than diffing because:
  //   1. The cost is one IPC per pack — Imported lists are tiny
  //   2. The diff is `is_modified`, which can flip from a Restore
  //      inside the drawer that doesn't propagate to ImportedView
  let statusMap = $state<Map<string, ModpackStatus | null>>(new Map());

  $effect(() => {
    // Subscribe to instances shape — refresh statuses when the list
    // changes (post-delete, post-import, post-restore via the parent's
    // refresh hook).
    const packIds = instances.filter((i) => i.mrpack_name != null).map((i) => i.id);
    void refreshStatuses(packIds);
  });

  async function refreshStatuses(ids: string[]) {
    const next = new Map<string, ModpackStatus | null>();
    await Promise.all(
      ids.map(async (id) => {
        const r = await commands.modpackStatus(id);
        if (r.status === 'ok') {
          next.set(id, r.data);
        }
      }),
    );
    statusMap = next;
  }

  const allPacks = $derived(instances.filter((i) => i.mrpack_name != null));

  const packs = $derived(
    allPacks
      .filter((i) => {
        if (query.trim()) {
          const q = query.toLowerCase();
          const name = (i.mrpack_name ?? '').toLowerCase();
          const iname = i.name.toLowerCase();
          if (!name.includes(q) && !iname.includes(q)) return false;
        }
        if (mcFilter && i.mc_version !== mcFilter) return false;
        if (loaderFilter && i.loader !== loaderFilter) return false;
        return true;
      })
      .sort((a, b) => {
        if (sort === 'newest') return (b.created_unix_ms ?? 0) - (a.created_unix_ms ?? 0);
        if (sort === 'oldest') return (a.created_unix_ms ?? 0) - (b.created_unix_ms ?? 0);
        return (a.mrpack_name ?? a.name).localeCompare(b.mrpack_name ?? b.name);
      }),
  );

  const mcOptions = $derived(
    Array.from(new Set(allPacks.map((i) => i.mc_version)))
      .sort()
      .reverse(),
  );
</script>

{#if allPacks.length === 0}
  <div class="p-4">
    <div class="text-sm text-placeholder text-center mt-12">
      No packs imported yet. Drop a .mrpack on the Browse tab or pick one from search.
    </div>
  </div>
{:else}
  <div class="p-4 pb-2 flex flex-wrap gap-2">
    <input
      type="search"
      bind:value={query}
      placeholder="Filter packs..."
      class="flex-1 min-w-[150px] px-3 py-1.5 border rounded text-sm"
      data-testid="imported-search-input"
    />
    <select
      bind:value={mcFilter}
      class="px-2 py-1.5 border rounded text-sm bg-surface"
      data-testid="imported-mc-filter"
    >
      <option value="">All MC versions</option>
      {#each mcOptions as v (v)}
        <option value={v}>{v}</option>
      {/each}
    </select>
    <select
      bind:value={loaderFilter}
      class="px-2 py-1.5 border rounded text-sm bg-surface"
      data-testid="imported-loader-filter"
    >
      <option value="">All loaders</option>
      <option value="fabric">Fabric</option>
      <option value="quilt">Quilt</option>
      <option value="forge">Forge</option>
      <option value="neoforge">NeoForge</option>
    </select>
    <select
      bind:value={sort}
      class="px-2 py-1.5 border rounded text-sm bg-surface"
      data-testid="imported-sort"
    >
      <option value="newest">Newest first</option>
      <option value="oldest">Oldest first</option>
      <option value="name">Name (A–Z)</option>
    </select>
  </div>

  <div class="p-4 pt-2 grid grid-cols-1 sm:grid-cols-2 gap-2">
    {#if packs.length === 0}
      <div class="col-span-full text-sm text-placeholder text-center mt-8">
        No packs match the filter.
      </div>
    {:else}
      {#each packs as inst (inst.id)}
        <ImportedCard
          {inst}
          onClick={() => (drawerInstId = inst.id)}
          isModified={statusMap.get(inst.id)?.is_modified ?? false}
        />
      {/each}
    {/if}
  </div>
{/if}

{#if drawerInst}
  <ImportedDetailDrawer
    inst={drawerInst}
    onClose={() => (drawerInstId = null)}
    onOpenInstance={(id) => {
      drawerInstId = null;
      onPick(id);
    }}
    onDeleted={() => {
      drawerInstId = null;
      onListChanged?.();
    }}
    onUpdated={() => onListChanged?.()}
  />
{/if}

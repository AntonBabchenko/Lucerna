<script lang="ts">
  import {
    commands,
    type LoaderKind,
    type LoaderVersion,
    type Error as IpcError,
  } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';

  function formatLoaderError(e: IpcError): string {
    if (e.kind === 'loader_unavailable')
      return `${displayLoader(e.loader as LoaderKind)} does not support Minecraft ${e.mc_version}`;
    if (e.kind === 'network') return `Network error fetching ${e.url}: ${e.details}`;
    if (e.kind === 'forge_promotions_unavailable')
      return `Forge promotions feed for ${e.flavor} is unavailable`;
    return JSON.stringify(e);
  }

  const LOADER_KINDS: LoaderKind[] = ['vanilla', 'fabric', 'quilt', 'forge', 'neoforge'];

  let {
    mc,
    loader = $bindable<LoaderKind>(),
    loaderVersion = $bindable<string | null>(),
    onchange,
    disabled = false,
  }: {
    mc: string;
    loader: LoaderKind;
    loaderVersion: string | null;
    onchange?: (loader: LoaderKind, version: string | null) => void;
    disabled?: boolean;
  } = $props();

  let versions = $state<LoaderVersion[]>([]);
  let error = $state<string | null>(null);

  // Refetch + auto-pick whenever (mc, loader) change. Auto-pick the
  // `stable` version if present, else the first entry. If the list is
  // empty (loader doesn't support this MC), clear loaderVersion so
  // submitters refuse to persist a broken combo.
  $effect(() => {
    const m = mc;
    const k = loader;
    if (k === 'vanilla' || !m) {
      versions = [];
      loaderVersion = null;
      // Clear any stale error from a previous (mc, loader) attempt —
      // without this, switching to vanilla (or wiping MC via openCreate)
      // leaves "Quilt does not support Minecraft 26.1.2" hanging below
      // the loader row from a prior failed pick.
      error = null;
      return;
    }
    void load(k, m);
  });

  async function load(k: LoaderKind, m: string) {
    error = null;
    const result =
      k === 'fabric'
        ? await commands.listFabricLoaders(m)
        : k === 'quilt'
          ? await commands.listQuiltLoaders(m)
          : k === 'neoforge'
            ? await commands.listNeoforgeLoaders(m)
            : await commands.listForgeLoaders(m);
    if (result.status === 'ok') {
      versions = result.data;
      const stable = result.data.find((l) => l.stable);
      loaderVersion = (stable ?? result.data[0])?.version ?? null;
    } else {
      versions = [];
      loaderVersion = null;
      error = formatLoaderError(result.error);
    }
  }

  function pickLoader(k: LoaderKind) {
    loader = k;
    // The $effect handles the refetch + auto-pick; we just emit the
    // user-initiated event so the parent can commit (detail-form path).
    onchange?.(k, loaderVersion);
  }

  function pickVersion(v: string) {
    loaderVersion = v;
    onchange?.(loader, v);
  }
</script>

<p class="block text-xs uppercase text-neutral-600 mb-1">Loader</p>
<div class="flex gap-1 mb-3">
  {#each LOADER_KINDS as lk}
    <button
      type="button"
      class="flex-1 border rounded px-2 py-1 text-xs"
      class:bg-blue-600={loader === lk}
      class:text-white={loader === lk}
      {disabled}
      onclick={() => pickLoader(lk)}
    >
      {displayLoader(lk)}
    </button>
  {/each}
</div>

{#if loader !== 'vanilla' && versions.length > 0}
  <label class="block text-xs uppercase text-neutral-600 mb-1" for="loader-version-select">
    Loader version
  </label>
  <select
    id="loader-version-select"
    class="border rounded px-2 py-1 w-full mb-3"
    value={loaderVersion ?? ''}
    {disabled}
    onchange={(e) => pickVersion((e.currentTarget as HTMLSelectElement).value)}
  >
    {#each versions as lv}
      <option value={lv.version}>
        {lv.version}{lv.stable ? ' (recommended)' : ''}
      </option>
    {/each}
  </select>
{/if}

{#if error}
  <p class="text-xs text-red-700 mb-2">{error}</p>
{/if}

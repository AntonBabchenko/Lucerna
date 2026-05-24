<script lang="ts">
  import {
    commands,
    type LoaderKind,
    type LoaderVersion,
    type Error as IpcError,
  } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';
  import { formatError } from '$lib/ipc/format-error';

  function formatLoaderError(e: IpcError): string {
    // Picker keeps shorter wording for the 3 variants it surfaces most
    // often; everything else delegates so no raw JSON if e.g. a loader
    // cache IO error reaches the picker.
    if (e.kind === 'loader_unavailable')
      return `${displayLoader(e.loader as LoaderKind)} does not support Minecraft ${e.mc_version}`;
    if (e.kind === 'network') return `Network error fetching ${e.url}: ${e.details}`;
    if (e.kind === 'forge_promotions_unavailable')
      return `Forge promotions feed for ${e.flavor} is unavailable`;
    return formatError(e);
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

  // Tracks the loader value the LAST $effect run dispatched a load() for.
  // Used to distinguish "the user just switched loader" (reset to new
  // loader's stable — explicit ecosystem change) from "same loader, just
  // a remount or MC tweak" (preserve the parent's pick if still valid).
  // Per-instance non-reactive let — only read/written inside $effect,
  // no consumer needs reactivity on it.
  let prevLoader: LoaderKind | undefined;

  // Refetch + auto-pick whenever (mc, loader) change. On loader switch:
  // reset to the new loader's stable. On mount / MC change: preserve
  // the parent's loaderVersion if it's in the fetched list, else fall
  // back to stable so the UI never shows a broken-combo selection.
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
      prevLoader = k;
      return;
    }
    const loaderChanged = prevLoader !== undefined && prevLoader !== k;
    prevLoader = k;
    void load(k, m, loaderChanged);
  });

  async function load(k: LoaderKind, m: string, resetToStable: boolean) {
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
      // resetToStable=true when this load was triggered by a user-driven
      // loader switch — pick the new ecosystem's stable regardless of
      // what loaderVersion happens to carry over (which is meaningless
      // for the new loader even when version numbers happen to overlap,
      // e.g. Fabric 0.16.0 and Quilt 0.16.0 are unrelated builds).
      // resetToStable=false on mount + on MC change — preserve the
      // parent's committed loaderVersion when still in the list, else
      // fall back to stable so the UI never shows a broken-combo.
      if (resetToStable) {
        const stable = result.data.find((l) => l.stable);
        loaderVersion = (stable ?? result.data[0])?.version ?? null;
      } else {
        const currentIsValid =
          loaderVersion != null && result.data.some((l) => l.version === loaderVersion);
        if (!currentIsValid) {
          const stable = result.data.find((l) => l.stable);
          loaderVersion = (stable ?? result.data[0])?.version ?? null;
        }
      }
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

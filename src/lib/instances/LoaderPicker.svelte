<script lang="ts">
  import { get } from 'svelte/store';
  import {
    commands,
    type LoaderKind,
    type LoaderVersion,
    type Error as IpcError,
  } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';
  import { resolveLoaderVersion } from '$lib/instances/loader-version';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';

  function formatLoaderError(e: IpcError): string {
    // Picker keeps shorter wording for the 3 variants it surfaces most
    // often; everything else delegates so no raw JSON if e.g. a loader
    // cache IO error reaches the picker.
    if (e.kind === 'loader_unavailable')
      return get(t)('instance.loader.errorUnavailable', {
        loader: displayLoader(e.loader as LoaderKind),
        mc: e.mc_version,
      });
    if (e.kind === 'network')
      return get(t)('instance.loader.errorNetwork', { url: e.url, details: e.details });
    if (e.kind === 'forge_promotions_unavailable')
      return get(t)('instance.loader.errorForgePromotions', { flavor: e.flavor });
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
      const next = resolveLoaderVersion(loaderVersion, result.data, resetToStable);
      if (next !== loaderVersion) {
        loaderVersion = next;
        // Commit the auto-correction: without this the dropdown would show
        // a valid version while a stale/invalid one stayed saved on the
        // instance, so Install kept fetching the broken (mc, fv) pair and
        // 404'd. Firing onchange persists the picker's pick. The guard
        // above means a load that changes nothing emits no event, so a
        // committed-then-reloaded value can't loop. (Create-form usage
        // passes no onchange → no-op.)
        onchange?.(k, next);
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

<p class="block text-xs uppercase text-secondary mb-1">{$t('instance.loader.label')}</p>
<div class="flex gap-1 mb-3">
  {#each LOADER_KINDS as lk}
    <button
      type="button"
      class="flex-1 btn-sm"
      class:btn-primary={loader === lk}
      class:btn-secondary={loader !== lk}
      {disabled}
      onclick={() => pickLoader(lk)}
    >
      {displayLoader(lk)}
    </button>
  {/each}
</div>

{#if loader !== 'vanilla' && versions.length > 0}
  <label class="block text-xs uppercase text-secondary mb-1" for="loader-version-select">
    {$t('instance.loader.versionLabel')}
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
        {lv.stable ? $t('instance.loader.recommended', { version: lv.version }) : lv.version}
      </option>
    {/each}
  </select>
{/if}

{#if error}
  <p class="text-xs text-danger mb-2">{error}</p>
{/if}

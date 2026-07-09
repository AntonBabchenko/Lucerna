<script lang="ts">
  import { get } from 'svelte/store';
  import {
    commands,
    type LoaderKind,
    type LoaderVersion,
    type ServerCore,
    type Error as IpcError,
  } from '$lib/ipc/bindings';
  import { displayCore, coreToLoaderKind } from '$lib/servers/core-display';
  import { displayLoader } from '$lib/instances/loader-display';
  import { resolveLoaderVersion } from '$lib/instances/loader-version';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import Select from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';

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

  const SERVER_CORES: ServerCore[] = [
    'vanilla',
    'fabric',
    'quilt',
    'forge',
    'neoforge',
    'paper',
    'purpur',
  ];
  // Unique per instance so two ServerCorePickers on a page never collide on
  // the group-label or version-select ids.
  const uid = crypto.randomUUID();
  const groupLabelId = `core-group-${uid}`;
  const versionSelectId = `core-version-${uid}`;

  let {
    mc,
    core = $bindable<ServerCore>(),
    coreVersion = $bindable<string | null>(),
    onchange,
    disabled = false,
  }: {
    mc: string;
    core: ServerCore;
    coreVersion: string | null;
    onchange?: (core: ServerCore, version: string | null) => void;
    disabled?: boolean;
  } = $props();

  let versions = $state<LoaderVersion[]>([]);
  let error = $state<string | null>(null);
  let isLoadingVersions = $state(false);

  // Tracks the core value the LAST $effect run dispatched a load() for.
  // Used to distinguish "the user just switched core" (reset to new
  // core's stable — explicit ecosystem change) from "same core, just
  // a remount or MC tweak" (preserve the parent's pick if still valid).
  // Per-instance non-reactive let — only read/written inside $effect,
  // no consumer needs reactivity on it.
  let prevCore: ServerCore | undefined;

  // Refetch + auto-pick whenever (mc, core) change. On core switch:
  // reset to the new core's stable. On mount / MC change: preserve
  // the parent's coreVersion if it's in the fetched list, else fall
  // back to stable so the UI never shows a broken-combo selection.
  //
  // Only the 4 mod-loader cores (fabric/quilt/forge/neoforge) have a
  // version list to fetch — vanilla has no version control, and
  // paper/purpur resolve their build server-side (no loader-version
  // Select at all, see markup below).
  $effect(() => {
    const m = mc;
    const c = core;
    const lk = coreToLoaderKind(c);
    if (lk === null || c === 'vanilla' || !m) {
      versions = [];
      coreVersion = null;
      // Clear any stale error from a previous (mc, core) attempt —
      // without this, switching to vanilla/paper/purpur (or wiping MC)
      // leaves an old "Quilt does not support Minecraft 26.1.2" hanging
      // below the core row from a prior failed pick.
      error = null;
      prevCore = c;
      return;
    }
    const coreChanged = prevCore !== undefined && prevCore !== c;
    prevCore = c;
    isLoadingVersions = true;
    void load(lk, m, coreChanged).finally(() => {
      isLoadingVersions = false;
    });
  });

  async function load(k: LoaderKind, m: string, resetToStable: boolean): Promise<void> {
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
      // core switch — pick the new ecosystem's stable regardless of what
      // coreVersion happens to carry over (which is meaningless for the
      // new core even when version numbers happen to overlap, e.g.
      // Fabric 0.16.0 and Quilt 0.16.0 are unrelated builds).
      // resetToStable=false on mount + on MC change — preserve the
      // parent's committed coreVersion when still in the list, else
      // fall back to stable so the UI never shows a broken-combo.
      const next = resolveLoaderVersion(coreVersion, result.data, resetToStable);
      if (next !== coreVersion) {
        coreVersion = next;
        // Commit the auto-correction: without this the dropdown would show
        // a valid version while a stale/invalid one stayed saved, so Create
        // kept sending the broken (mc, version) pair. Firing onchange
        // persists the picker's pick. The guard above means a load that
        // changes nothing emits no event, so a committed-then-reloaded
        // value can't loop.
        onchange?.(k, next);
      }
    } else {
      versions = [];
      coreVersion = null;
      error = formatLoaderError(result.error);
    }
  }

  function pickCore(c: ServerCore) {
    core = c;
    // The $effect handles the refetch + auto-pick for mod loaders, and
    // resets to null for vanilla/paper/purpur. We emit the user-initiated
    // event immediately so the parent can commit; paper/purpur have no
    // version control so the version is always null.
    onchange?.(c, coreToLoaderKind(c) === null ? null : coreVersion);
  }

  function pickVersion(v: string) {
    coreVersion = v;
    onchange?.(core, v);
  }

  // Mirror LoaderPicker's markup: stable entries carry the "(recommended)"
  // suffix, non-stable show the bare version. Recomputed whenever the
  // fetched `versions` list changes.
  const versionOptions = $derived(
    versions.map((lv) => ({
      value: lv.version,
      label: lv.stable ? $t('instance.loader.recommended', { version: lv.version }) : lv.version,
    })),
  );
</script>

<p id={groupLabelId} class="block text-xs uppercase text-secondary mb-1">
  {$t('instance.loader.label')}
</p>
<div class="flex flex-wrap gap-1 mb-3" role="group" aria-labelledby={groupLabelId}>
  {#each SERVER_CORES as sc}
    <button
      type="button"
      class="flex-1 btn-sm"
      class:btn-primary={core === sc}
      class:btn-secondary={core !== sc}
      aria-pressed={core === sc}
      {disabled}
      onclick={() => pickCore(sc)}
    >
      {displayCore(sc)}
    </button>
  {/each}
</div>

{#if coreToLoaderKind(core) !== null && core !== 'vanilla' && (versions.length > 0 || isLoadingVersions)}
  <label class="block text-xs uppercase text-secondary mb-1" for={versionSelectId}>
    {$t('instance.loader.versionLabel')}
  </label>
  {#if isLoadingVersions}
    <div class="w-full mb-3 flex items-center gap-2 text-secondary">
      <Spinner
        size="sm"
        labelPlacement="right"
        label={$t('instance.loader.loadingVersions')}
        delayMs={150}
      />
    </div>
  {:else}
    <Select
      id={versionSelectId}
      class="w-full mb-3"
      value={coreVersion ?? ''}
      options={versionOptions}
      {disabled}
      onChange={(v) => pickVersion(String(v))}
    />
  {/if}
{:else if core === 'paper' || core === 'purpur'}
  <p class="text-xs text-muted mb-3">{$t('servers.core.latestBuildHint')}</p>
{/if}

<StatusMessage tone="danger" message={error} class="mb-2" />

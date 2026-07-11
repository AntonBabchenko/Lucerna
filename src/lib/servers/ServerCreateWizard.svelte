<script lang="ts">
  import {
    commands,
    type InstanceWithStatus,
    type ServerCore,
    type VersionEntry,
  } from '$lib/ipc/bindings';
  import { get } from 'svelte/store';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { modCapable, pluginCapable } from '$lib/servers/core-display';
  import { formatHeapLabel } from '$lib/instances/heap';
  import MemorySlider from '$lib/instances/MemorySlider.svelte';
  import ServerCorePicker from '$lib/servers/ServerCorePicker.svelte';
  import { serverState } from '$lib/servers/server-state.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import ServerImportView from '$lib/servers/ServerImportView.svelte';

  let {
    instances,
    versions,
    onDone,
    onCancel,
  }: {
    instances: InstanceWithStatus[];
    versions: VersionEntry[];
    // Passes the new server's id on success so the host can auto-select it
    // (the import path may omit it — callers must handle `undefined`).
    onDone: (createdId?: string) => void;
    onCancel: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let mode = $state<'instance' | 'standalone' | 'import'>(
    instances.length > 0 ? 'instance' : 'standalone',
  );
  let name = $state('');
  // svelte-ignore state_referenced_locally
  let instanceId = $state<string | null>(instances.length > 0 ? instances[0].id : null);
  let mcVersion = $state('');
  let core = $state<ServerCore>('vanilla');
  let coreVersion = $state<string | null>(null);
  let eula = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  // Snapshots hidden by default — most users want stable releases. Mirrors the
  // instance create form's MC-version picker.
  let showSnapshots = $state(false);
  const visibleVersions = $derived(
    versions.filter((v) => (showSnapshots ? true : v.version_type === 'release')),
  );

  // Paper/Purpur publish builds for a subset of MC versions only. Fetch the
  // core's version catalogue once per core selection (seq-guarded, mirrors
  // ServerPluginBrowser's reload pattern) and filter the MC dropdown down to
  // the intersection so the user can't pick a combo the backend would 404 on.
  let coreVersions = $state<Set<string> | null>(null);
  let coreVersionsError = $state<string | null>(null);
  let coreVersionsLoading = $state(false);
  let coreVerSeq = 0;

  $effect(() => {
    const c = core;
    // Bump the seq on EVERY effect run (before the branch check) so any
    // in-flight plugin-core fetch is invalidated the moment core changes.
    // Without this, switching Paper→Fabric would take the early-return
    // branch (seq unchanged) and let Paper's still-pending fetch resolve
    // through the guard, wrongly narrowing the MC dropdown to Paper's set
    // for a Fabric server. Mirrors ServerPluginBrowser's reqSeq (increment
    // at the start of every run, no leave-branch gap).
    const seq = ++coreVerSeq;
    if (!pluginCapable(c)) {
      coreVersions = null;
      coreVersionsError = null;
      coreVersionsLoading = false;
      return;
    }
    coreVersionsLoading = true;
    coreVersionsError = null;
    void commands.serverCoreVersions(c).then((res) => {
      if (seq !== coreVerSeq) return; // stale
      coreVersionsLoading = false;
      if (res.status === 'ok') {
        coreVersions = new Set(res.data);
      } else {
        coreVersions = null;
        coreVersionsError = formatError(res.error);
      }
    });
  });

  const coreFilteredVersions = $derived.by(() => {
    const allowed = coreVersions;
    return allowed === null ? visibleVersions : visibleVersions.filter((v) => allowed.has(v.id));
  });

  // When a plugin core's catalogue finishes loading and the currently-picked
  // MC version isn't in its supported set, drop the dangling selection. This
  // collapses an out-of-intersection pick back to the normal empty-mcVersion
  // state (the dropdown now offers only supported versions, user re-picks) —
  // so canCreate/disabledReason never has to reason about an "unsupported MC"
  // state. Guarded to the loaded-without-error case: during loading/error the
  // dropdown still shows every version and must not be cleared. Reads
  // coreVersions + coreVersionsError + coreFilteredVersions + mcVersion,
  // writes only mcVersion.
  $effect(() => {
    if (coreVersions === null || coreVersionsError !== null) return;
    const current = mcVersion;
    if (current !== '' && !coreFilteredVersions.some((v) => v.id === current)) {
      mcVersion = '';
    }
  });

  const mcVersionOptions = $derived([
    { value: '', label: $t('instance.manage.chooseMcOption') },
    ...coreFilteredVersions.map((v) => ({ value: v.id, label: v.id })),
  ]);

  // Heap for the new server. The adaptive bounds live inside MemorySlider.
  let memoryMb = $state(4096);

  const instanceOptions = $derived(
    instances.map((i) => ({
      value: i.id,
      label: `${i.name} · ${displayLoader(i.loader)} ${i.mc_version}`,
    })),
  );

  // Standalone-mode core selection is valid once: a MC version is chosen,
  // mod-loader cores (fabric/quilt/forge/neoforge) have a resolved loader
  // version, and paper/purpur have successfully loaded their build
  // catalogue with the chosen MC version inside the intersection (vanilla
  // needs neither — coreVersion stays null throughout).
  const standaloneCoreReady = $derived(
    modCapable(core)
      ? coreVersion !== null
      : pluginCapable(core)
        ? coreVersions !== null && coreVersionsError === null && coreVersions.has(mcVersion)
        : true,
  );

  const canCreate = $derived(
    name.trim().length > 0 &&
      eula &&
      (mode === 'instance'
        ? instanceId !== null
        : mcVersion.trim().length > 0 && standaloneCoreReady),
  );

  // Tell the user WHY Create is disabled instead of leaving a dead button
  // (#21-FE). Mirrors canCreate's checks in the same order so the first missing
  // requirement is named; null once everything is satisfied.
  const disabledReason = $derived.by<string | null>(() => {
    if (name.trim().length === 0) return $t('servers.wizard.disabledReason.name');
    if (mode === 'instance') {
      if (instanceId === null) return $t('servers.wizard.disabledReason.instance');
    } else {
      // Plugin cores (paper/purpur) have no loader-version field, so the
      // generic "choose a loader version" reason would misdirect. Surface
      // their distinct blocking states first: catalogue loading, catalogue
      // error, then fall through to the standard empty-mcVersion prompt.
      // (An out-of-intersection MC is auto-cleared to '' before it reaches
      // here, so it collapses into the empty-mcVersion case — no separate
      // "unsupported MC" reason is needed.)
      if (pluginCapable(core)) {
        if (coreVersionsLoading) return $t('servers.wizard.coreVersionsLoading');
        if (coreVersionsError !== null) return coreVersionsError;
        if (mcVersion.trim().length === 0) return $t('servers.wizard.disabledReason.version');
      } else {
        if (mcVersion.trim().length === 0) return $t('servers.wizard.disabledReason.version');
        // Only the four mod-loader cores have a loader-version to choose.
        if (!standaloneCoreReady) {
          return $t('servers.wizard.disabledReason.loader');
        }
      }
    }
    if (!eula) return $t('servers.wizard.disabledReason.eula');
    return null;
  });

  async function handleCreate() {
    if (!canCreate || busy) return;

    let effectiveMcVersion: string;
    let effectiveLoader: ServerCore;
    let effectiveLoaderVersion: string | null;
    let createdFromInstance: string | null;

    if (mode === 'instance') {
      const inst = instances.find((i) => i.id === instanceId);
      if (!inst) return;
      effectiveMcVersion = inst.mc_version;
      effectiveLoader = inst.loader;
      effectiveLoaderVersion = inst.loader_version;
      createdFromInstance = inst.id;
    } else {
      effectiveMcVersion = mcVersion.trim();
      effectiveLoader = core;
      effectiveLoaderVersion = coreVersion;
      createdFromInstance = null;
    }

    busy = true;
    error = null;
    try {
      const res = await commands.serverCreate(
        name.trim(),
        effectiveMcVersion,
        effectiveLoader,
        effectiveLoaderVersion,
        memoryMb,
        true,
        createdFromInstance,
      );
      if (res.status === 'ok') {
        await serverState.refresh();
        // Summary: if client-only mods were set aside so the server can start.
        const setAside = res.data.quarantined.length;
        if (setAside > 0) {
          pushSuccess(get(t)('servers.diagnose.quarantined', { count: setAside }));
        }
        onDone(res.data.server.id);
      } else {
        error = formatError(res.error);
      }
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex flex-col gap-4 p-4 overflow-y-auto">
  <h2 class="text-lg font-semibold">{$t('servers.wizard.title')}</h2>

  <!-- Mode toggle -->
  <div class="grid grid-cols-3 gap-2">
    <button
      type="button"
      class="flex flex-col items-start gap-1 rounded-lg border p-3 text-left transition-colors"
      class:border-accent={mode === 'instance'}
      class:bg-accent-soft={mode === 'instance'}
      class:border-border-subtle={mode !== 'instance'}
      class:opacity-50={instances.length === 0}
      class:cursor-not-allowed={instances.length === 0}
      disabled={instances.length === 0}
      onclick={() => {
        if (instances.length > 0) mode = 'instance';
      }}
    >
      <span class="text-sm font-medium">{$t('servers.wizard.fromInstance')}</span>
      <span class="text-xs text-muted">{$t('servers.wizard.fromInstanceHint')}</span>
    </button>
    <button
      type="button"
      class="flex flex-col items-start gap-1 rounded-lg border p-3 text-left transition-colors"
      class:border-accent={mode === 'standalone'}
      class:bg-accent-soft={mode === 'standalone'}
      class:border-border-subtle={mode !== 'standalone'}
      onclick={() => (mode = 'standalone')}
    >
      <span class="text-sm font-medium">{$t('servers.wizard.standalone')}</span>
      <span class="text-xs text-muted">{$t('servers.wizard.standaloneHint')}</span>
    </button>
    <button
      type="button"
      aria-label={$t('servers.import.mode')}
      class="flex flex-col items-start gap-1 rounded-lg border p-3 text-left transition-colors"
      class:border-accent={mode === 'import'}
      class:bg-accent-soft={mode === 'import'}
      class:border-border-subtle={mode !== 'import'}
      onclick={() => (mode = 'import')}
    >
      <span class="text-sm font-medium">{$t('servers.import.mode')}</span>
      <span class="text-xs text-muted">{$t('servers.import.modeHint')}</span>
    </button>
  </div>

  {#if mode === 'import'}
    <!-- Import mode: ServerImportView owns the rest of the flow -->
    <ServerImportView {onDone} {onCancel} />
  {:else}
    <!-- Name -->
    <div class="flex flex-col gap-1">
      <div class="flex items-center justify-between">
        <label for="wizard-name" class="text-sm font-medium">{$t('servers.wizard.name')}</label>
        <span class="text-xs text-muted">{name.length}/32</span>
      </div>
      <input
        id="wizard-name"
        type="text"
        maxlength="32"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={name}
      />
    </div>

    {#if mode === 'instance'}
      <!-- Instance selector -->
      <div class="flex flex-col gap-1">
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label class="text-sm font-medium">{$t('servers.wizard.instance')}</label>
        <Select
          value={instanceId}
          options={instanceOptions}
          onChange={(v) => (instanceId = String(v))}
          ariaLabel={$t('servers.wizard.instance')}
        />
      </div>
    {:else}
      <!-- Standalone: MC version (dropdown) + loader (with its own version) -->
      <div class="flex flex-col gap-1">
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label class="text-sm font-medium">{$t('servers.wizard.version')}</label>
        <Select
          value={mcVersion}
          options={mcVersionOptions}
          onChange={(v) => (mcVersion = String(v))}
          ariaLabel={$t('servers.wizard.version')}
        />
        <label class="flex items-center gap-1 text-xs">
          <input type="checkbox" bind:checked={showSnapshots} />
          {$t('instance.manage.showSnapshots')}
        </label>
        {#if coreVersionsLoading}
          <Spinner
            size="sm"
            labelPlacement="right"
            label={$t('servers.wizard.coreVersionsLoading')}
            delayMs={150}
          />
        {:else if coreVersionsError}
          <p class="text-xs text-danger">{$t('servers.wizard.coreVersionsError')}</p>
        {/if}
      </div>
      <div class="flex flex-col gap-1">
        <ServerCorePicker
          mc={mcVersion}
          {core}
          {coreVersion}
          onchange={(c, v) => {
            core = c;
            coreVersion = v;
          }}
        />
      </div>
    {/if}

    <!-- Memory: adaptive slider (same control as instance settings) -->
    <div class="flex flex-col gap-1">
      <!-- svelte-ignore a11y_label_has_associated_control -->
      <label class="text-sm font-medium"
        >{$t('servers.wizard.memory')} · {formatHeapLabel(memoryMb)}</label
      >
      <MemorySlider valueMb={memoryMb} onInput={(mb) => (memoryMb = mb)} />
    </div>

    <!-- EULA -->
    <label class="flex items-start gap-2 cursor-pointer">
      <input
        type="checkbox"
        class="mt-0.5 flex-shrink-0"
        bind:checked={eula}
        aria-label={$t('servers.wizard.eula')}
      />
      <span class="flex flex-col gap-0.5">
        <span class="text-sm">{$t('servers.wizard.eula')}</span>
        <span class="text-xs text-muted">{$t('servers.wizard.eulaRequired')}</span>
      </span>
    </label>

    {#if error}
      <p class="text-sm text-danger">{error}</p>
    {/if}

    <!-- Actions -->
    <div class="flex items-center justify-between gap-2">
      {#if disabledReason}
        <span class="text-xs text-muted" data-testid="wizard-disabled-reason">
          {disabledReason}
        </span>
      {:else}
        <span></span>
      {/if}
      <div class="flex gap-2">
        <button type="button" class="btn-secondary btn-sm" onclick={onCancel}>
          {$t('servers.wizard.cancel')}
        </button>
        <BusyButton class="btn-primary btn-sm" {busy} disabled={!canCreate} onclick={handleCreate}>
          {$t('servers.wizard.create')}
        </BusyButton>
      </div>
    </div>
  {/if}
</div>

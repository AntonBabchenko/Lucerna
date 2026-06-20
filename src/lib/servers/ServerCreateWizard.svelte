<script lang="ts">
  import {
    commands,
    type InstanceWithStatus,
    type LoaderKind,
    type MemoryBounds,
    type VersionEntry,
  } from '$lib/ipc/bindings';
  import { get } from 'svelte/store';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { formatHeapLabel, isAboveRecommended } from '$lib/instances/heap';
  import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
  import { serverState } from '$lib/servers/server-state.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Select from '$lib/ui/Select.svelte';
  import ServerImportView from '$lib/servers/ServerImportView.svelte';

  let {
    instances,
    versions,
    onDone,
    onCancel,
  }: {
    instances: InstanceWithStatus[];
    versions: VersionEntry[];
    onDone: () => void;
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
  let loader = $state<LoaderKind>('vanilla');
  let loaderVersion = $state<string | null>(null);
  let eula = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  // Snapshots hidden by default — most users want stable releases. Mirrors the
  // instance create form's MC-version picker.
  let showSnapshots = $state(false);
  const visibleVersions = $derived(
    versions.filter((v) => (showSnapshots ? true : v.version_type === 'release')),
  );
  const mcVersionOptions = $derived([
    { value: '', label: $t('instance.manage.chooseMcOption') },
    ...visibleVersions.map((v) => ({ value: v.id, label: v.id })),
  ]);

  // Adaptive memory bounds — same source/control as instance create. Static
  // fallback keeps the slider usable before the command resolves or if it fails.
  const FALLBACK_BOUNDS: MemoryBounds = {
    min_mb: 1024,
    max_mb: 8192,
    recommended_max_mb: 8192,
    step_mb: 256,
    ram_known: false,
  };
  let memBounds = $state<MemoryBounds>(FALLBACK_BOUNDS);
  let memoryMb = $state(4096);
  let memBoundsLoaded = false;
  $effect(() => {
    if (memBoundsLoaded) return;
    memBoundsLoaded = true;
    commands
      .instanceMemoryBounds()
      .then((b) => {
        memBounds = b;
        // Clamp the default into this machine's real adaptive range.
        memoryMb = Math.min(Math.max(memoryMb, b.min_mb), b.max_mb);
      })
      .catch(() => {
        // Graceful degradation: keep FALLBACK_BOUNDS so the slider stays usable.
      });
  });

  const instanceOptions = $derived(
    instances.map((i) => ({
      value: i.id,
      label: `${i.name} · ${displayLoader(i.loader)} ${i.mc_version}`,
    })),
  );

  const canCreate = $derived(
    name.trim().length > 0 &&
      eula &&
      (mode === 'instance'
        ? instanceId !== null
        : mcVersion.trim().length > 0 && (loader === 'vanilla' || loaderVersion !== null)),
  );

  // Tell the user WHY Create is disabled instead of leaving a dead button
  // (#21-FE). Mirrors canCreate's checks in the same order so the first missing
  // requirement is named; null once everything is satisfied.
  const disabledReason = $derived.by<string | null>(() => {
    if (name.trim().length === 0) return $t('servers.wizard.disabledReason.name');
    if (mode === 'instance') {
      if (instanceId === null) return $t('servers.wizard.disabledReason.instance');
    } else {
      if (mcVersion.trim().length === 0) return $t('servers.wizard.disabledReason.version');
      if (loader !== 'vanilla' && loaderVersion === null) {
        return $t('servers.wizard.disabledReason.loader');
      }
    }
    if (!eula) return $t('servers.wizard.disabledReason.eula');
    return null;
  });

  async function handleCreate() {
    if (!canCreate || busy) return;

    let effectiveMcVersion: string;
    let effectiveLoader: LoaderKind;
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
      effectiveLoader = loader;
      effectiveLoaderVersion = loaderVersion;
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
        onDone();
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
      </div>
      <div class="flex flex-col gap-1">
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label class="text-sm font-medium">{$t('servers.wizard.loader')}</label>
        <LoaderPicker
          mc={mcVersion}
          {loader}
          {loaderVersion}
          onchange={(l, v) => {
            loader = l;
            loaderVersion = v;
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
      <input
        type="range"
        min={memBounds.min_mb}
        max={memBounds.max_mb}
        step={memBounds.step_mb}
        value={memoryMb}
        oninput={(e) => (memoryMb = parseInt((e.currentTarget as HTMLInputElement).value, 10))}
        class="w-full"
      />
      {#if isAboveRecommended(memoryMb, memBounds.recommended_max_mb, memBounds.ram_known)}
        <p class="text-xs text-warning-text">
          {$t('instance.manage.memoryWarnHigh', {
            recommended: formatHeapLabel(memBounds.recommended_max_mb),
          })}
        </p>
      {/if}
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
        <button type="button" class="btn-ghost btn-sm" onclick={onCancel}>
          {$t('servers.wizard.cancel')}
        </button>
        <BusyButton class="btn-primary btn-sm" {busy} disabled={!canCreate} onclick={handleCreate}>
          {$t('servers.wizard.create')}
        </BusyButton>
      </div>
    </div>
  {/if}
</div>

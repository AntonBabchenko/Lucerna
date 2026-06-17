<script lang="ts">
  import { commands, type InstanceWithStatus, type LoaderKind } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { displayLoader } from '$lib/instances/loader-display';
  import { serverState } from '$lib/servers/server-state.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Select from '$lib/ui/Select.svelte';

  let {
    instances,
    onDone,
    onCancel,
  }: {
    instances: InstanceWithStatus[];
    onDone: () => void;
    onCancel: () => void;
  } = $props();

  const ALL_LOADERS: LoaderKind[] = ['vanilla', 'fabric', 'quilt', 'forge', 'neoforge'];

  // svelte-ignore state_referenced_locally
  let mode = $state<'instance' | 'standalone'>(instances.length > 0 ? 'instance' : 'standalone');
  let name = $state('');
  // svelte-ignore state_referenced_locally
  let instanceId = $state<string | null>(instances.length > 0 ? instances[0].id : null);
  let mcVersion = $state('');
  let loader = $state<LoaderKind>('fabric');
  let loaderVersion = $state('');
  let memoryMb = $state(4096);
  let eula = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const instanceOptions = $derived(
    instances.map((i) => ({
      value: i.id,
      label: `${i.name} · ${displayLoader(i.loader)} ${i.mc_version}`,
    })),
  );

  const loaderOptions = $derived(ALL_LOADERS.map((k) => ({ value: k, label: displayLoader(k) })));

  const canCreate = $derived(
    name.trim().length > 0 &&
      eula &&
      (mode === 'instance' ? instanceId !== null : mcVersion.trim().length > 0),
  );

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
      effectiveLoaderVersion = loaderVersion.trim() || null;
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
  <div class="grid grid-cols-2 gap-2">
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
  </div>

  <!-- Name -->
  <div class="flex flex-col gap-1">
    <label for="wizard-name" class="text-sm font-medium">{$t('servers.wizard.name')}</label>
    <input
      id="wizard-name"
      type="text"
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
    <!-- Standalone: MC version, loader, loader version -->
    <div class="flex flex-col gap-1">
      <label for="wizard-mc-version" class="text-sm font-medium"
        >{$t('servers.wizard.version')}</label
      >
      <input
        id="wizard-mc-version"
        type="text"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={mcVersion}
        placeholder="1.21.1"
      />
    </div>
    <div class="flex flex-col gap-1">
      <!-- svelte-ignore a11y_label_has_associated_control -->
      <label class="text-sm font-medium">{$t('servers.wizard.loader')}</label>
      <Select
        value={loader}
        options={loaderOptions}
        onChange={(v) => (loader = v as LoaderKind)}
        ariaLabel={$t('servers.wizard.loader')}
      />
    </div>
    <div class="flex flex-col gap-1">
      <label for="wizard-loader-version" class="text-sm font-medium"
        >{$t('servers.wizard.loaderVersion')}</label
      >
      <input
        id="wizard-loader-version"
        type="text"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={loaderVersion}
      />
    </div>
  {/if}

  <!-- Memory -->
  <div class="flex flex-col gap-1">
    <label for="wizard-memory" class="text-sm font-medium">{$t('servers.wizard.memory')}</label>
    <input
      id="wizard-memory"
      type="number"
      min="512"
      step="256"
      class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
      bind:value={memoryMb}
    />
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
  <div class="flex justify-end gap-2">
    <button type="button" class="btn-ghost btn-sm" onclick={onCancel}>
      {$t('servers.wizard.cancel')}
    </button>
    <BusyButton class="btn-primary btn-sm" {busy} disabled={!canCreate} onclick={handleCreate}>
      {$t('servers.wizard.create')}
    </BusyButton>
  </div>
</div>

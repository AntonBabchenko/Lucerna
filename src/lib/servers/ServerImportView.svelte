<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { commands, type LoaderKind, type MemoryBounds } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { formatHeapLabel, isAboveRecommended } from '$lib/instances/heap';
  import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
  import { serverState } from '$lib/servers/server-state.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import FileDropzone from '$lib/mods/FileDropzone.svelte';
  import { droppedServer, serverImportActive, dragActive } from '$lib/settings/state.svelte';

  let { onDone, onCancel }: { onDone: () => void; onCancel: () => void } = $props();

  // Phase: 'pick' → source selection, 'confirm' → review + commit.
  let phase = $state<'pick' | 'confirm'>('pick');
  let busy = $state(false);
  let error = $state<string | null>(null);

  // Token from the inspect response; cleared after a successful commit so
  // destroy doesn't attempt to cancel a finished import.
  let token = $state<string | null>(null);

  // Editable confirm-step fields, prefilled from the inspect preview.
  let name = $state('');
  let mcVersion = $state('');
  let loader = $state<LoaderKind>('vanilla');
  let loaderVersion = $state<string | null>(null);
  let eula = $state(false);
  let canLaunchAsIs = $state(false);
  let modCount = $state(0);
  let worldPresent = $state(false);
  // The loader couldn't be auto-detected, so it was defaulted to Vanilla (#20).
  // Surfaced as a warning until the user picks a real loader — otherwise mods
  // silently won't load.
  let loaderUnknown = $state(false);

  // Adaptive memory bounds — same pattern as ServerCreateWizard.
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
        memoryMb = Math.min(Math.max(memoryMb, b.min_mb), b.max_mb);
      })
      .catch(() => {
        // Graceful degradation: keep FALLBACK_BOUNDS.
      });
  });

  // Mark the import view as active on mount; register own drag-drop listener.
  onMount(() => {
    serverImportActive.value = true;
    let cleanup: (() => void) | null = null;

    // Lazily import the Tauri webview API so test environments (happy-dom)
    // don't crash — the webview API is unavailable outside Tauri.
    import('@tauri-apps/api/webview')
      .then(({ getCurrentWebview }) => {
        const pending = getCurrentWebview().onDragDropEvent((event) => {
          const payload = (event as { payload: { type: string; paths?: string[] } }).payload;
          const evType = payload.type;
          if (evType === 'enter' || evType === 'over') {
            dragActive.value = true;
          } else if (evType === 'leave') {
            dragActive.value = false;
          } else if (evType === 'drop') {
            dragActive.value = false;
            const paths = payload.paths ?? [];
            if (paths.length > 0) {
              droppedServer.value = paths;
            }
          }
        });
        pending.then((un) => {
          cleanup = un;
        });
      })
      .catch(() => {
        // Not in a Tauri context (tests / SSR); skip.
      });

    return () => {
      cleanup?.();
    };
  });

  // On destroy: deactivate, cancel any pending inspect if not committed.
  onDestroy(() => {
    serverImportActive.value = false;
    if (token !== null) {
      void serverState.importCancel(token);
    }
  });

  // Consume droppedServer paths set by our own drag-drop listener.
  $effect(() => {
    const v = droppedServer.value;
    if (v !== null && v.length > 0) {
      droppedServer.value = null;
      void doInspect(v[0]);
    }
  });

  async function doInspect(path: string): Promise<void> {
    busy = true;
    error = null;
    try {
      const r = await serverState.importInspect(path);
      if (!r.ok) {
        error = formatError(r.error as Parameters<typeof formatError>[0]);
      } else if (r.preview) {
        token = r.preview.token;
        name = r.preview.detected_name;
        mcVersion = r.preview.mc_version ?? '';
        loaderUnknown = r.preview.loader === null;
        loader = (r.preview.loader as LoaderKind | null) ?? 'vanilla';
        loaderVersion = r.preview.loader_version ?? null;
        canLaunchAsIs = r.preview.can_launch_as_is;
        eula = r.preview.eula_in_source;
        modCount = r.preview.mod_count;
        worldPresent = r.preview.world_present;
        phase = 'confirm';
      }
    } finally {
      busy = false;
    }
  }

  async function pickZip(): Promise<void> {
    const picked = await openFile({
      multiple: false,
      filters: [{ name: 'Server zip', extensions: ['zip'] }],
    });
    if (typeof picked === 'string') await doInspect(picked);
  }

  async function pickFolder(): Promise<void> {
    const picked = await openFile({ directory: true });
    if (typeof picked === 'string') await doInspect(picked);
  }

  async function doImport(): Promise<void> {
    if (!name.trim() || !eula || !token) return;
    busy = true;
    error = null;
    try {
      const effectiveLoaderVersion = loader === 'vanilla' ? null : loaderVersion;
      const r = await serverState.importCommit(
        token,
        name.trim(),
        mcVersion.trim(),
        loader,
        effectiveLoaderVersion,
        memoryMb,
        eula,
      );
      if (r.ok) {
        token = null; // prevent destroy from cancelling a completed import
        onDone();
      } else {
        error = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } finally {
      busy = false;
    }
  }

  async function goBack(): Promise<void> {
    if (token !== null) {
      void serverState.importCancel(token);
      token = null;
    }
    phase = 'pick';
    error = null;
  }

  const canImport = $derived(name.trim().length > 0 && eula);
</script>

{#if phase === 'pick'}
  <div class="flex flex-col gap-4 p-4">
    <FileDropzone label={$t('servers.import.dropzone')} onClick={() => void pickZip()} />

    <div class="flex gap-2">
      <button type="button" class="btn-secondary btn-sm" onclick={() => void pickZip()}>
        {$t('servers.import.pickZip')}
      </button>
      <button type="button" class="btn-secondary btn-sm" onclick={() => void pickFolder()}>
        {$t('servers.import.pickFolder')}
      </button>
    </div>

    {#if busy}
      <p class="text-sm text-muted">{$t('servers.import.inspecting')}</p>
    {/if}

    {#if error}
      <p class="text-sm text-danger">{error}</p>
    {/if}

    <div class="flex justify-end">
      <button type="button" class="btn-ghost btn-sm" onclick={onCancel}>
        {$t('common.cancel')}
      </button>
    </div>
  </div>
{:else}
  <!-- Confirm step -->
  <div class="flex flex-col gap-4 p-4 overflow-y-auto">
    <p class="text-xs text-muted">{$t('servers.import.detected')}</p>

    <!-- Preserve / reprovision badge -->
    <p
      class="text-xs rounded px-2 py-1"
      class:bg-success-bg={canLaunchAsIs}
      class:text-success-text={canLaunchAsIs}
      class:bg-warning-bg={!canLaunchAsIs}
      class:text-warning-text={!canLaunchAsIs}
    >
      {canLaunchAsIs ? $t('servers.import.willPreserve') : $t('servers.import.willReprovision')}
    </p>

    <!-- Mod count / world summary -->
    <p class="text-xs text-muted">
      {$t('servers.import.modCount', { count: modCount })}{#if worldPresent}
        · {$t('servers.import.worldPresent')}{/if}
    </p>

    <!-- Name -->
    <div class="flex flex-col gap-1">
      <label for="import-name" class="text-sm font-medium">{$t('servers.wizard.name')}</label>
      <input
        id="import-name"
        aria-label="Name"
        type="text"
        maxlength="32"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={name}
      />
    </div>

    <!-- MC version -->
    <div class="flex flex-col gap-1">
      <label for="import-mc-version" class="text-sm font-medium">
        {$t('servers.wizard.version')}
      </label>
      <input
        id="import-mc-version"
        type="text"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={mcVersion}
      />
    </div>

    <!-- Loader -->
    <div class="flex flex-col gap-1">
      <!-- svelte-ignore a11y_label_has_associated_control -->
      <label class="text-sm font-medium">{$t('servers.wizard.loader')}</label>
      {#if loaderUnknown && loader === 'vanilla'}
        <p
          class="rounded bg-warning-bg px-2 py-1 text-xs text-warning-text"
          role="alert"
          data-testid="import-loader-unknown-warn"
        >
          {$t('servers.import.loaderUnknownWarn')}
        </p>
      {/if}
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

    <!-- Memory -->
    <div class="flex flex-col gap-1">
      <!-- svelte-ignore a11y_label_has_associated_control -->
      <label class="text-sm font-medium">
        {$t('servers.wizard.memory')} · {formatHeapLabel(memoryMb)}
      </label>
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
    <div class="flex justify-between gap-2">
      <button type="button" class="btn-ghost btn-sm" onclick={() => void goBack()}>
        {$t('servers.import.back')}
      </button>
      <BusyButton
        class="btn-primary btn-sm"
        {busy}
        disabled={!canImport}
        onclick={() => void doImport()}
      >
        {busy ? $t('servers.import.importing') : $t('servers.import.import')}
      </BusyButton>
    </div>
  </div>
{/if}

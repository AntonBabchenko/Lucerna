<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { type ServerCore } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { formatHeapLabel } from '$lib/instances/heap';
  import MemorySlider from '$lib/instances/MemorySlider.svelte';
  import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
  import { displayCore, pluginCapable, coreToLoaderKind } from '$lib/servers/core-display';
  import { serverState } from '$lib/servers/server-state.svelte';
  import EulaLink from '$lib/servers/EulaLink.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import FileDropzone from '$lib/mods/FileDropzone.svelte';
  import { droppedServer, serverImportActive, dragActive } from '$lib/settings/state.svelte';

  let {
    onDone,
    onCancel,
  }: {
    // Passes the new server's id on success so the host can auto-select it.
    onDone: (createdId?: string) => void;
    onCancel: () => void;
  } = $props();

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
  let loader = $state<ServerCore>('vanilla');
  let loaderVersion = $state<string | null>(null);
  let eula = $state(false);
  let canLaunchAsIs = $state(false);
  let modCount = $state(0);
  let worldPresent = $state(false);
  // The loader couldn't be auto-detected, so it was defaulted to Vanilla (#20).
  // Surfaced as a warning until the user picks a real loader — otherwise mods
  // silently won't load.
  let loaderUnknown = $state(false);

  // Heap for the imported server. The adaptive bounds live inside MemorySlider.
  let memoryMb = $state(4096);

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
        error = formatError(r.error);
      } else {
        token = r.preview.token;
        name = r.preview.detected_name;
        mcVersion = r.preview.mc_version ?? '';
        loaderUnknown = r.preview.loader === null;
        loader = r.preview.loader ?? 'vanilla';
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
      filters: [{ name: $t('common.fileFilter.serverZip'), extensions: ['zip'] }],
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
      // Vanilla and plugin cores (paper/purpur) have no loader version to
      // pick — the backend resolves paper/purpur builds server-side.
      const effectiveLoaderVersion =
        loader === 'vanilla' || pluginCapable(loader) ? null : loaderVersion;
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
        onDone(r.server.id);
      } else {
        error = formatError(r.error);
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

  // LoaderPicker only understands the 5 mod-loader kinds; paper/purpur are
  // shown read-only below instead (see the loader section markup). Deriving
  // through the shared coreToLoaderKind map (rather than an inline cast)
  // keeps this in sync with pluginCapable/modCapable's classification and
  // narrows to LoaderKind for TypeScript.
  const loaderKind = $derived(coreToLoaderKind(loader));
</script>

{#if phase === 'pick'}
  <div class="flex flex-col gap-4 p-4">
    <FileDropzone label={$t('servers.import.dropzone')} onClick={() => void pickZip()} />

    <div class="flex gap-2">
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={busy}
        onclick={() => void pickZip()}
      >
        {$t('servers.import.pickZip')}
      </button>
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={busy}
        onclick={() => void pickFolder()}
      >
        {$t('servers.import.pickFolder')}
      </button>
    </div>

    {#if busy}
      <Spinner size="sm" labelPlacement="right" label={$t('servers.import.inspecting')} />
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
      class:text-success={canLaunchAsIs}
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

    <!-- Core / loader -->
    <div class="flex flex-col gap-1">
      {#if loaderKind === null}
        <!-- Plugin cores (Paper/Purpur) are server CORES, not mod loaders, and
             have no loader-version to pick; overriding a detected plugin-core
             import to a mod loader is out of scope here (LoaderPicker is
             LoaderKind-only) — show the detected core read-only, under a "Server
             core" label (LoaderPicker's own "Loader" label doesn't render on
             this branch, so this static <p> needs one). -->
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label class="text-sm font-medium">{$t('servers.core.sectionTitle')}</label>
        <p
          class="h-8 flex items-center rounded border border-border-subtle bg-surface px-3 text-sm text-primary"
        >
          {displayCore(loader)}
        </p>
        <p class="text-xs text-muted">{$t('servers.core.latestBuildHint')}</p>
      {:else}
        <!-- Mod-loader cores (incl. vanilla): LoaderPicker renders its own
             internal "Loader" group label, so no wrapping field label here
             (mirrors ManageInstancesModal's single-label mount). The
             unknown-vanilla warn belongs on this branch: coreToLoaderKind
             ('vanilla') is 'vanilla' (non-null), so this is where it fires. -->
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
          loader={loaderKind}
          {loaderVersion}
          onchange={(l, v) => {
            loader = l;
            loaderVersion = v;
          }}
        />
      {/if}
    </div>

    <!-- Memory -->
    <div class="flex flex-col gap-1">
      <!-- svelte-ignore a11y_label_has_associated_control -->
      <label class="text-sm font-medium">
        {$t('servers.wizard.memory')} · {formatHeapLabel($t, memoryMb)}
      </label>
      <MemorySlider valueMb={memoryMb} onInput={(mb) => (memoryMb = mb)} />
    </div>

    <!-- EULA. Same shape as the create wizard: the link lives outside the
         <label> so reading the agreement never accepts it. -->
    <div class="flex items-start gap-2">
      <input
        id="server-import-eula"
        type="checkbox"
        class="mt-0.5 flex-shrink-0"
        bind:checked={eula}
        aria-label={$t('servers.wizard.eula')}
      />
      <span class="flex flex-col gap-0.5">
        <span class="text-sm">
          <label for="server-import-eula" class="cursor-pointer"
            >{$t('servers.wizard.eulaPrefix')}</label
          >
          <EulaLink />
        </span>
        <span class="text-xs text-muted">{$t('servers.wizard.eulaRequired')}</span>
      </span>
    </div>

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

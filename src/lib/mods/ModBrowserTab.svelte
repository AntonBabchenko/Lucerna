<script lang="ts">
  import type { CompatVerdict, ModSource } from '$lib/ipc/bindings';
  import { modBrowserNav } from '$lib/settings/state.svelte';
  import InstalledModsView from './InstalledModsView.svelte';
  import ModBrowseView from './ModBrowseView.svelte';
  import SourcePicker from './SourcePicker.svelte';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { droppedMods } from '$lib/settings/state.svelte';
  import { canInstallMods } from './install-eligibility';
  import CompatWarningDialog from './CompatWarningDialog.svelte';
  import FileDropzone from './FileDropzone.svelte';

  type View = 'browse' | 'installed';

  let view = $state<View>('browse');
  let source = $state<ModSource>('modrinth');

  // Once a sub-tab is opened we keep it mounted (hidden via CSS when
  // not active) so the user's filters, search query, pagination etc.
  // survive switching back and forth. Without this each switch
  // re-mounts the view component and resets its $state.
  let browseMounted = $state(true); // Browse is the default — mount immediately.
  let installedMounted = $state(false);
  $effect(() => {
    if (view === 'browse') browseMounted = true;
    if (view === 'installed') installedMounted = true;
  });

  // Cross-component navigation from Overview: open the Installed
  // sub-view directly. Resets the rune so subsequent in-tab clicks
  // aren't hijacked.
  $effect(() => {
    if (modBrowserNav.value !== null) {
      view = modBrowserNav.value.view;
      modBrowserNav.value = null;
    }
  });

  // Props come from +page.svelte's activeInstance and are forwarded to
  // ModBrowseView (Task 14) and InstalledModsView (Task 17). When no
  // instance is selected the Browse pane still works for read-only
  // browsing — only Install needs all three, and InstalledModsView
  // renders its own "Pick an instance first" empty state.
  let {
    instanceId,
    mcVersion,
    loader,
  }: {
    instanceId: string | null;
    mcVersion: string | null;
    loader: 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;
  } = $props();

  // Local-mod install (the drag-drop droppedMods consumer + the "Install
  // from file…" button) is available only for a selected, non-vanilla
  // instance. Same rule as MainTabs' drag-drop router — shared via
  // canInstallMods() so it is defined once.
  const installDisabled = $derived(!canInstallMods(instanceId, loader));

  // Files dropped on the Mods tab arrive via the droppedMods rune
  // (routed by MainTabs). Consume and reset so a later action isn't
  // re-triggered.
  $effect(() => {
    const v = droppedMods.value;
    if (v !== null) {
      droppedMods.value = null;
      void onJarsPicked(v);
    }
  });

  async function installFromFile() {
    if (installDisabled) return;
    const r = await openFile({
      multiple: true,
      filters: [{ name: 'Mod jar', extensions: ['jar'] }],
    });
    if (Array.isArray(r) && r.length > 0) await onJarsPicked(r);
  }

  type PendingJar = { path: string; filename: string };
  type MismatchRow = { filename: string; reason: string };
  let mismatchRows = $state<MismatchRow[]>([]);
  let pendingCompatible = $state<PendingJar[]>([]);
  let pendingMismatched = $state<PendingJar[]>([]);

  function filenameOf(path: string): string {
    return path.split(/[\\/]/).pop() ?? path;
  }

  function mismatchReason(v: CompatVerdict): string {
    const parts: string[] = [];
    if (v.loader_mismatch && v.detected_loader) {
      parts.push(`looks like a ${v.detected_loader} mod, instance is ${loader}`);
    }
    if (v.mc_mismatch && v.detected_mc) {
      parts.push(`targets MC ${v.detected_mc}, instance is ${mcVersion}`);
    }
    return parts.join('; ') || 'may not be compatible';
  }

  async function onJarsPicked(paths: string[]) {
    if (instanceId === null) return;
    const compatible: PendingJar[] = [];
    const mismatched: PendingJar[] = [];
    const rows: MismatchRow[] = [];
    for (const path of paths) {
      const filename = filenameOf(path);
      const r = await commands.modsInspectLocal(instanceId, path);
      if (r.status !== 'ok') {
        pushWarning(`Could not read ${filename}`, [formatError(r.error)]);
        continue;
      }
      const v = r.data;
      if (v.loader_mismatch || v.mc_mismatch) {
        mismatched.push({ path, filename });
        rows.push({ filename, reason: mismatchReason(v) });
      } else {
        compatible.push({ path, filename });
      }
    }
    if (rows.length > 0) {
      pendingCompatible = compatible;
      pendingMismatched = mismatched;
      mismatchRows = rows;
    } else {
      await installJars(compatible);
    }
  }

  async function installJars(jars: PendingJar[]) {
    if (instanceId === null || jars.length === 0) return;
    let ok = 0;
    const failed: string[] = [];
    for (const j of jars) {
      const r = await commands.modsInstallLocal(instanceId, j.path);
      if (r.status === 'ok') ok += 1;
      else failed.push(`${j.filename}: ${formatError(r.error)}`);
    }
    if (ok > 0) pushSuccess(`Installed ${ok} mod${ok === 1 ? '' : 's'}`);
    if (failed.length > 0) pushWarning(`${failed.length} mod(s) failed to install`, failed);
  }

  async function confirmInstallAll() {
    const all = [...pendingCompatible, ...pendingMismatched];
    mismatchRows = [];
    pendingCompatible = [];
    pendingMismatched = [];
    await installJars(all);
  }

  async function cancelMismatched() {
    const compatible = pendingCompatible;
    const skipped = pendingMismatched.map((j) => j.filename);
    mismatchRows = [];
    pendingCompatible = [];
    pendingMismatched = [];
    await installJars(compatible);
    if (skipped.length > 0) pushWarning(`Skipped ${skipped.length} incompatible mod(s)`, skipped);
  }
</script>

<div class="flex flex-col h-full">
  <!-- Sub-tab row. Underline style — matches the Modpacks tab's
       Browse/Imported sub-tabs and the top-level tab row. -->
  <div class="flex items-center justify-between px-3 border-b border-neutral-200 bg-white">
    <div role="tablist" class="flex gap-1">
      <button
        type="button"
        role="tab"
        aria-selected={view === 'browse'}
        class="px-3 py-2 text-sm border-b-2 -mb-px"
        class:border-blue-600={view === 'browse'}
        class:font-semibold={view === 'browse'}
        class:border-transparent={view !== 'browse'}
        class:text-neutral-400={view !== 'browse'}
        onclick={() => (view = 'browse')}
      >
        Browse
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={view === 'installed'}
        class="px-3 py-2 text-sm border-b-2 -mb-px"
        class:border-blue-600={view === 'installed'}
        class:font-semibold={view === 'installed'}
        class:border-transparent={view !== 'installed'}
        class:text-neutral-400={view !== 'installed'}
        onclick={() => (view = 'installed')}
      >
        Installed
      </button>
    </div>
    <SourcePicker bind:value={source} />
  </div>

  <div class="px-3 pt-3">
    <FileDropzone
      label="Drop a mod .jar here to install — or click to browse"
      disabled={installDisabled}
      disabledLabel="Select a non-vanilla instance to install mods"
      onClick={installFromFile}
    />
  </div>

  <div class="flex-1 overflow-y-auto relative">
    {#if browseMounted}
      <div class:hidden={view !== 'browse'}>
        <ModBrowseView {source} {instanceId} {mcVersion} {loader} />
      </div>
    {/if}
    {#if installedMounted}
      <div class:hidden={view !== 'installed'}>
        <InstalledModsView {instanceId} {mcVersion} {loader} />
      </div>
    {/if}
  </div>
</div>

{#if mismatchRows.length > 0}
  <CompatWarningDialog rows={mismatchRows} onConfirm={confirmInstallAll} onCancel={cancelMismatched} />
{/if}

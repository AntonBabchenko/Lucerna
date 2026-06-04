<script lang="ts">
  import type { CompatVerdict, ContentKind, ModSource } from '$lib/ipc/bindings';
  import { modBrowserNav } from '$lib/settings/state.svelte';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import InstalledModsView from './InstalledModsView.svelte';
  import InstalledAssetsView from './InstalledAssetsView.svelte';
  import ModBrowseView from './ModBrowseView.svelte';
  import SourcePicker from './SourcePicker.svelte';
  import SegmentedControl from '$lib/browse/SegmentedControl.svelte';
  import { CONTENT_KINDS } from './content-kind';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { droppedMods } from '$lib/settings/state.svelte';
  import { canInstallMods } from './install-eligibility';
  import { get } from 'svelte/store';
  import CompatWarningDialog from './CompatWarningDialog.svelte';
  import FileDropzone from './FileDropzone.svelte';

  type View = 'browse' | 'installed';

  // The content kind this Add-ons tab is currently showing. Switching it
  // re-keys the Browse view (clean filters/results) and swaps the Installed
  // sub-view between the mods view and the assets view. The default is 'mod'
  // so the historical Mod-browser experience is unchanged.
  let kind = $state<ContentKind>('mod');
  let view = $state<View>('browse');
  let source = $state<ModSource>('modrinth');

  // i18n labels for the kind switch — order mirrors CONTENT_KINDS.
  const kindLabels: Record<ContentKind, TranslationKey> = {
    mod: 'addons.kindMods',
    resource_pack: 'addons.kindResourcePacks',
    shader: 'addons.kindShaders',
  };
  const kindOptions = $derived(CONTENT_KINDS.map((k) => ({ value: k, label: $t(kindLabels[k]) })));

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
  // sub-view directly. Only applies to mods (the Overview link is
  // "Installed mods"); we leave `kind` untouched so the mod path stays
  // intact. Resets the rune so subsequent in-tab clicks aren't hijacked.
  $effect(() => {
    if (modBrowserNav.value !== null) {
      view = modBrowserNav.value.view;
      modBrowserNav.value = null;
    }
  });

  // Props come from +page.svelte's activeInstance and are forwarded to
  // ModBrowseView and the Installed sub-view. When no instance is selected
  // the Browse pane still works for read-only browsing — only Install needs
  // all three, and the Installed views render their own empty states.
  let {
    instanceId,
    instanceName = null,
    mcVersion,
    loader,
  }: {
    instanceId: string | null;
    instanceName?: string | null;
    mcVersion: string | null;
    loader: 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;
  } = $props();

  // Local-jar install (the drag-drop droppedMods consumer + the "Install
  // from file…" button) is MOD-ONLY and available only for a selected,
  // non-vanilla instance. Same rule as MainTabs' drag-drop router — shared
  // via canInstallMods() so it is defined once.
  const installDisabled = $derived(!canInstallMods(instanceId, loader));

  // Files dropped on the Mods tab arrive via the droppedMods rune
  // (routed by MainTabs). Consume and reset so a later action isn't
  // re-triggered. Guarded to kind='mod': a jar dropped while a non-mod
  // segment is active is ignored (cleared without acting).
  $effect(() => {
    const v = droppedMods.value;
    if (v !== null) {
      droppedMods.value = null;
      if (kind === 'mod') void onJarsPicked(v);
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
    const translate = get(t);
    const parts: string[] = [];
    if (v.loader_mismatch && v.detected_loader) {
      parts.push(
        translate('mods.browse.mismatchLoader', {
          detected: v.detected_loader,
          instance: loader ?? '',
        }),
      );
    }
    if (v.mc_mismatch && v.detected_mc) {
      parts.push(
        translate('mods.browse.mismatchMc', { detected: v.detected_mc, instance: mcVersion ?? '' }),
      );
    }
    return parts.join('; ') || translate('mods.browse.mismatchDefault');
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
        pushWarning(get(t)('mods.browse.toastCouldNotRead', { filename }), [formatError(r.error)]);
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
    if (ok > 0) pushSuccess(get(t)('mods.browse.toastInstalled', { count: ok }));
    if (failed.length > 0)
      pushWarning(get(t)('mods.browse.toastFailedToInstall', { count: failed.length }), failed);
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
    if (skipped.length > 0)
      pushWarning(get(t)('mods.browse.toastSkipped', { count: skipped.length }), skipped);
  }
</script>

<div class="flex flex-col h-full">
  <!-- Content-kind switch: Mods · Resource packs · Shaders. Sits above the
       Browse/Installed sub-tab row so it scopes everything below it. -->
  <div class="px-3 pt-3">
    <SegmentedControl
      value={kind}
      options={kindOptions}
      ariaLabel={$t('addons.kindSwitchAria')}
      testid="addons-kind-switch"
      onChange={(v) => (kind = v as ContentKind)}
    />
  </div>

  <!-- Sub-tab row. Underline style — matches the Modpacks tab's
       Browse/Imported sub-tabs and the top-level tab row. -->
  <div class="flex items-center justify-between px-3 border-b border-border-subtle bg-surface mt-3">
    <div role="tablist" class="flex gap-1">
      <button
        type="button"
        role="tab"
        aria-selected={view === 'browse'}
        class="px-3 py-2 text-sm border-b-2 -mb-px"
        class:border-accent={view === 'browse'}
        class:text-primary={view === 'browse'}
        class:font-semibold={view === 'browse'}
        class:border-transparent={view !== 'browse'}
        class:text-placeholder={view !== 'browse'}
        onclick={() => (view = 'browse')}
      >
        {$t('mods.browse.tabBrowse')}
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={view === 'installed'}
        class="px-3 py-2 text-sm border-b-2 -mb-px"
        class:border-accent={view === 'installed'}
        class:text-primary={view === 'installed'}
        class:font-semibold={view === 'installed'}
        class:border-transparent={view !== 'installed'}
        class:text-placeholder={view !== 'installed'}
        onclick={() => (view = 'installed')}
      >
        {$t('mods.browse.tabInstalled')}
      </button>
    </div>
    <SourcePicker value={source} onChange={(v) => (source = v)} />
  </div>

  {#if kind === 'shader'}
    <!-- Non-blocking info banner: shaders need a shader loader to run. -->
    <div class="px-3 pt-3">
      <div
        class="bg-accent/10 border border-accent/40 text-secondary text-sm rounded p-2"
        role="note"
      >
        {$t('addons.shaderLoaderHint')}
      </div>
    </div>
  {/if}

  {#if kind === 'mod'}
    <div class="px-3 pt-3">
      <FileDropzone
        label={$t('mods.browse.dropzoneLabel')}
        disabled={installDisabled}
        disabledLabel={$t('mods.browse.dropzoneDisabled')}
        onClick={installFromFile}
      />
    </div>
  {/if}

  <div class="flex-1 overflow-y-auto relative">
    {#if browseMounted}
      <div class:hidden={view !== 'browse'}>
        <!-- Re-key per kind so switching content type resets the browse
             filters/results instead of leaking the previous kind's state. -->
        {#key kind}
          <ModBrowseView {kind} {source} {instanceId} {instanceName} {mcVersion} {loader} />
        {/key}
      </div>
    {/if}
    {#if installedMounted}
      <div class:hidden={view !== 'installed'}>
        {#if kind === 'mod'}
          <InstalledModsView {instanceId} {mcVersion} {loader} />
        {:else}
          <InstalledAssetsView {instanceId} {kind} />
        {/if}
      </div>
    {/if}
  </div>
</div>

{#if mismatchRows.length > 0}
  <CompatWarningDialog
    rows={mismatchRows}
    onConfirm={confirmInstallAll}
    onCancel={cancelMismatched}
  />
{/if}

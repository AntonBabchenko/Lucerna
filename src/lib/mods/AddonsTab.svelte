<script lang="ts">
  import type { CompatVerdict, ContentKind, ModSource } from '$lib/ipc/bindings';
  import {
    modBrowseOpenProject,
    modBrowserNav,
    addonsKind,
    droppedMods,
    droppedAssets,
    assetsChanged,
  } from '$lib/settings/state.svelte';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { Icon, type IconName } from '$lib/ui/icons';
  import InstalledModsView from './InstalledModsView.svelte';
  import InstalledAssetsView from './InstalledAssetsView.svelte';
  import ModBrowseView from './ModBrowseView.svelte';
  import SourcePicker from './SourcePicker.svelte';
  import TabBar from '$lib/ui/TabBar.svelte';
  import { CONTENT_KINDS, canInstallContent } from './content-kind';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { canInstallMods } from './install-eligibility';
  import { get } from 'svelte/store';
  import CompatWarningDialog from './CompatWarningDialog.svelte';
  import FileDropzone from './FileDropzone.svelte';
  import { onDestroy, untrack } from 'svelte';

  type View = 'browse' | 'installed';

  // The content kind this Add-ons tab is currently showing. Switching it
  // re-keys the Browse view (clean filters/results) and swaps the Installed
  // sub-view between the mods view and the assets view. The default is 'mod'
  // so the historical Mod-browser experience is unchanged.
  let kind = $state<ContentKind>('mod');
  let view = $state<View>('browse');
  let source = $state<ModSource>('modrinth');

  // Iris's canonical Modrinth project id (the base62 id, not the "iris" slug)
  // so the detail modal's installed-state match and the project cache line up
  // with how mod cards key the same project. OptiFine is distributed only from
  // its own site — no Modrinth/CurseForge install path — so its action opens
  // the downloads page in the system browser.
  const IRIS_MODRINTH_PROJECT_ID = 'YL57xq9U';
  const OPTIFINE_DOWNLOADS_URL = 'https://optifine.net/downloads';

  // Shader-loader hint actions. Iris is a Fabric/Quilt mod: open it in the
  // Mods segment's detail modal, fully reusing ModBrowseView's dependency-aware
  // install flow. Switching `kind` to 'mod' re-keys ModBrowseView, and the
  // freshly-mounted mod browser consumes modBrowseOpenProject to open Iris.
  function openIris() {
    source = 'modrinth';
    kind = 'mod';
    view = 'browse';
    modBrowseOpenProject.value = { source: 'modrinth', projectId: IRIS_MODRINTH_PROJECT_ID };
  }

  function openOptifine() {
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(OPTIFINE_DOWNLOADS_URL));
  }

  // Mirror the local kind into the cross-component addonsKind rune so the
  // MainTabs drag-drop router knows which segment is active without needing
  // a prop chain. Resets to 'mod' when this component is destroyed (i.e. when
  // the user navigates away from the Add-ons tab) so a stale kind does not
  // poison a future drop on a different tab.
  $effect(() => {
    addonsKind.value = kind;
  });

  // The Iris deep-link rune is module-level and outlives this component. If the
  // tab is torn down between openIris() and the mod browser consuming it, clear
  // it so a later mount can't open Iris unprompted.
  onDestroy(() => {
    modBrowseOpenProject.value = null;
    addonsKind.value = 'mod';
  });

  // i18n labels for the kind switch — order mirrors CONTENT_KINDS.
  const kindLabels: Record<ContentKind, TranslationKey> = {
    mod: 'addons.kindMods',
    resource_pack: 'addons.kindResourcePacks',
    shader: 'addons.kindShaders',
  };
  const kindIcons: Record<ContentKind, IconName> = {
    mod: 'blocks',
    resource_pack: 'resourcePack',
    shader: 'shader',
  };
  const kindOptions = $derived(
    CONTENT_KINDS.map((k) => ({ value: k, label: $t(kindLabels[k]), icon: kindIcons[k] })),
  );

  // Compat-warning dialog state — declared early so the kind-change
  // derived below can reference them.
  type PendingJar = { path: string; filename: string };
  type MismatchRow = { filename: string; reason: string };
  let mismatchRows = $state<MismatchRow[]>([]);
  let pendingCompatible = $state<PendingJar[]>([]);
  let pendingMismatched = $state<PendingJar[]>([]);

  // True while a local-jar install (modsInstallLocal) is in flight. Drives the
  // busy state on the compat dialog's install button so the user gets feedback.
  let installingLocal = $state(false);

  // Tracks which kinds the user has explicitly opened the Installed sub-tab
  // for. This is the source of truth for lazy-mount: the Installed pane is
  // only mounted when the user has opened it for the CURRENT kind, which
  // prevents premature IPC on kind switch.
  let installedOpenedForKind = $state(new Set<ContentKind>());

  // Cross-component navigation from Overview: open the Installed
  // sub-view directly. Only applies to mods (the Overview link is
  // "Installed mods"); we leave `kind` untouched so the mod path stays
  // intact. Resets the rune so subsequent in-tab clicks aren't hijacked.
  $effect(() => {
    if (modBrowserNav.value !== null) {
      view = modBrowserNav.value.view;
      if (modBrowserNav.value.view === 'installed') {
        installedOpenedForKind = new Set([...installedOpenedForKind, kind]);
      }
      modBrowserNav.value = null;
    }
  });

  // When kind changes, reset to Browse so the new kind always starts on the
  // browse sub-tab, and clear any compat-dialog state left over from mods.
  // `prevKind` is intentionally non-reactive (not $state) and seeded with
  // `untrack` to read the initial kind without subscribing — this lets the
  // guard skip the first render (which may have been pre-set to 'installed'
  // by the modBrowserNav effect above) while still triggering on later
  // kind changes.
  let prevKind = untrack(() => kind);
  $effect(() => {
    const currentKind = kind; // subscribe to kind
    if (currentKind !== prevKind) {
      prevKind = currentKind;
      view = 'browse';
      // Clear any compat-warning dialog state — a mismatch dialog left open
      // on Mods must not remain actionable after switching to another kind,
      // and stale state must not reappear if the user switches back to Mods.
      mismatchRows = [];
      pendingCompatible = [];
      pendingMismatched = [];
    }
  });

  // Lazy-mount: Installed pane is only rendered once the user has explicitly
  // opened it for the current kind. This prevents premature IPC when switching
  // kind while the Installed tab had been open for a previous kind.
  // Browse is always mounted (it is the default landing sub-tab).
  const installedMounted = $derived(installedOpenedForKind.has(kind));

  // When the user clicks a sub-tab, arm the mount flag and switch the view.
  function selectView(v: View) {
    view = v;
    if (v === 'installed') {
      installedOpenedForKind = new Set([...installedOpenedForKind, kind]);
    }
  }

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

  // Resource packs / shaders install onto any selected instance (reuses the
  // shared content rule; for non-mod kinds it is simply "an instance is selected").
  const assetInstallDisabled = $derived(!canInstallContent(kind, instanceId, loader));

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

  // Zips dropped on a Resource-pack/Shader segment arrive via droppedAssets
  // (routed by MainTabs). Consume and reset; guard that the payload kind still
  // matches the active segment (the user may have switched kinds mid-drag).
  $effect(() => {
    const v = droppedAssets.value;
    if (v !== null) {
      droppedAssets.value = null;
      if (v.kind === kind && kind !== 'mod') void installAssetsFromFiles(v.paths);
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

  async function installAssetsFromPicker() {
    if (assetInstallDisabled) return;
    const r = await openFile({
      multiple: true,
      filters: [{ name: 'Add-on zip', extensions: ['zip'] }],
    });
    if (Array.isArray(r) && r.length > 0) await installAssetsFromFiles(r);
  }

  async function installAssetsFromFiles(paths: string[]) {
    if (instanceId === null) return;
    let ok = 0;
    const failed: string[] = [];
    for (const path of paths) {
      const r = await commands.assetInstallLocal(instanceId, kind, path);
      if (r.status === 'ok') ok += 1;
      else failed.push(`${filenameOf(path)}: ${formatError(r.error)}`);
    }
    if (ok > 0) pushSuccess(get(t)('addons.install.toastInstalled', { count: ok }));
    if (failed.length > 0)
      pushWarning(get(t)('addons.install.toastFailed', { count: failed.length }), failed);
    // Refresh the Installed view + Browse badges (assets have no Tauri events).
    if (ok > 0) assetsChanged.value++;
  }

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
    installingLocal = true;
    try {
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
    } finally {
      installingLocal = false;
    }
  }

  async function confirmInstallAll() {
    // Keep the dialog mounted while the install runs so its button can show
    // the busy state; clear the pending state only once the install resolves.
    const all = [...pendingCompatible, ...pendingMismatched];
    await installJars(all);
    mismatchRows = [];
    pendingCompatible = [];
    pendingMismatched = [];
  }

  async function cancelMismatched() {
    // Keep the dialog mounted while the compatible jars install so its button
    // can show the busy state; clear the pending state once it resolves.
    const compatible = pendingCompatible;
    const skipped = pendingMismatched.map((j) => j.filename);
    await installJars(compatible);
    mismatchRows = [];
    pendingCompatible = [];
    pendingMismatched = [];
    if (skipped.length > 0)
      pushWarning(get(t)('mods.browse.toastSkipped', { count: skipped.length }), skipped);
  }
</script>

<div class="flex flex-col h-full">
  <!-- Content-kind switch: Mods · Resource packs · Shaders. Underline tabs (the
       app-wide TabBar style) so it reads as the primary scope above the
       Browse/Installed sub-tab row, not a clashing boxed segmented control. -->
  <div class="px-3 pt-1">
    <TabBar
      tabs={kindOptions.map((o) => ({ id: o.value, label: o.label, icon: o.icon }))}
      active={kind}
      ariaLabel={$t('addons.kindSwitchAria')}
      testid="addons-kind-switch"
      onChange={(id) => (kind = id as ContentKind)}
    />
  </div>

  <!-- Sub-tab row. Underline style — matches the Modpacks tab's
       Browse/Imported sub-tabs and the top-level tab row. -->
  <div class="flex items-center justify-between px-3 border-b border-border-subtle bg-surface mt-3">
    <div role="tablist" aria-label={$t('addons.subTabsLabel')} class="flex gap-1">
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
        onclick={() => selectView('browse')}
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
        onclick={() => selectView('installed')}
      >
        {$t('mods.browse.tabInstalled')}
      </button>
    </div>
    <SourcePicker value={source} onChange={(v) => (source = v)} />
  </div>

  {#if kind === 'shader'}
    <!-- Non-blocking info banner: shaders need a shader loader to run. Iris is
         actionable (opens its mod detail modal); OptiFine opens its downloads
         page since it has no in-app install path. -->
    <div class="px-3 pt-3">
      <div
        class="bg-accent/10 border border-accent/40 text-secondary text-sm rounded p-2 space-y-1.5"
        role="note"
      >
        <p>{$t('addons.shaderLoaderHint.intro')}</p>
        <p class="flex flex-wrap items-center gap-x-2 gap-y-1">
          <button
            type="button"
            class="font-medium text-accent underline underline-offset-2 hover:no-underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2 rounded"
            onclick={openIris}
          >
            {$t('addons.shaderLoaderHint.iris')}
          </button>
          <span class="text-placeholder">{$t('addons.shaderLoaderHint.or')}</span>
          <button
            type="button"
            class="inline-flex items-center gap-1 font-medium text-accent underline underline-offset-2 hover:no-underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2 rounded"
            onclick={openOptifine}
          >
            {$t('addons.shaderLoaderHint.optifine')}<Icon name="externalLink" size={14} />
          </button>
        </p>
        <p class="text-xs text-muted">
          {#if mcVersion}
            {$t('addons.shaderLoaderHint.optifineVersion', { mc: mcVersion })}
          {:else}
            {$t('addons.shaderLoaderHint.optifineNoInstance')}
          {/if}
        </p>
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

  {#if kind === 'resource_pack' || kind === 'shader'}
    <div class="px-3 pt-3">
      <FileDropzone
        label={kind === 'resource_pack'
          ? $t('addons.dropzoneResourcePack')
          : $t('addons.dropzoneShader')}
        disabled={assetInstallDisabled}
        disabledLabel={$t('addons.dropzoneDisabled')}
        onClick={installAssetsFromPicker}
      />
    </div>
  {/if}

  <div class="flex-1 overflow-y-auto relative">
    <div class:hidden={view !== 'browse'}>
      <!-- Re-key per kind so switching content type resets the browse
           filters/results instead of leaking the previous kind's state. -->
      {#key kind}
        <ModBrowseView {kind} {source} {instanceId} {instanceName} {mcVersion} {loader} />
      {/key}
    </div>
    {#if installedMounted}
      <div class:hidden={view !== 'installed'}>
        {#if kind === 'mod'}
          <InstalledModsView {instanceId} {mcVersion} {loader} />
        {:else}
          <InstalledAssetsView {instanceId} {kind} {mcVersion} {loader} />
        {/if}
      </div>
    {/if}
  </div>
</div>

{#if mismatchRows.length > 0}
  <CompatWarningDialog
    rows={mismatchRows}
    busy={installingLocal}
    onConfirm={confirmInstallAll}
    onCancel={cancelMismatched}
  />
{/if}

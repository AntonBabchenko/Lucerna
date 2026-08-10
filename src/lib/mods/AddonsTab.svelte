<script module lang="ts">
  // Session-lived answers for the 1.13 datapack gate, keyed by instanceId
  // and mcVersion joined with a newline — a character that can appear in
  // neither half, so the key can never be ambiguous. Module-level so it
  // survives the remount AddonsTab goes through on every top-level tab
  // switch — see the supportsDatapacks effect for why that matters.
  const supportsDatapacksCache = new Map<string, boolean>();
</script>

<script lang="ts">
  import type {
    CompatVerdict,
    ContentKind,
    DatapackPlacementView,
    InstalledMod,
    ModSource,
  } from '$lib/ipc/bindings';
  import {
    modBrowseOpenProject,
    modBrowserNav,
    addonsKind,
    droppedMods,
    droppedAssets,
    assetsChanged,
    datapacksChanged,
  } from '$lib/settings/state.svelte';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { Icon, type IconName } from '$lib/ui/icons';
  import InstalledModsView from './installed/InstalledModsView.svelte';
  import InstalledAssetsView from './InstalledAssetsView.svelte';
  import InstalledDatapacksView from './InstalledDatapacksView.svelte';
  import DatapackWorldPicker from './DatapackWorldPicker.svelte';
  import ModBrowseView from './ModBrowseView.svelte';
  import SourcePicker from './SourcePicker.svelte';
  import TabBar from '$lib/ui/TabBar.svelte';
  import { rainbowFx } from '$lib/fx/rainbow-fx.svelte';
  import { CONTENT_KINDS, canInstallContent, type InstanceContentKind } from './content-kind';
  import {
    detectInstalledShaderLoaders,
    IRIS_MODRINTH_PROJECT_ID,
    OCULUS_MODRINTH_PROJECT_ID,
    shaderLoaderOptions,
  } from './shader-loader-hint';
  import { commands, events } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { canInstallMods } from './install-eligibility';
  import { get } from 'svelte/store';
  import CompatWarningDialog from './CompatWarningDialog.svelte';
  import FileDropzone from './FileDropzone.svelte';
  import { onDestroy, untrack } from 'svelte';
  import { listenUntilDestroyed } from '$lib/ipc/listen';
  import { debounceTrailing } from '$lib/ui/debounce';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { ADDONS_STEPS } from '$lib/onboarding/contextual-tours';

  type View = 'browse' | 'installed';

  // The content kind this Add-ons tab is currently showing. Switching it
  // re-keys the Browse view (clean filters/results) and swaps the Installed
  // sub-view between the mods view and the assets view. The default is 'mod'
  // so the historical Mod-browser experience is unchanged.
  let kind = $state<InstanceContentKind>('mod');
  let view = $state<View>('browse');
  let source = $state<ModSource>('modrinth');

  // Iris's canonical Modrinth project id (the base62 id, not the "iris" slug)
  // so the detail modal's installed-state match and the project cache line up
  // with how mod cards key the same project. OptiFine is distributed only from
  // its own site — no Modrinth/CurseForge install path — so its action opens
  // the downloads page in the system browser.
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

  // Oculus is the Forge/NeoForge port of Iris. Same deep-link mechanism as
  // openIris: flip to the Mods segment and let the freshly-mounted mod browser
  // consume modBrowseOpenProject to open Oculus's detail modal.
  function openOculus() {
    source = 'modrinth';
    kind = 'mod';
    view = 'browse';
    modBrowseOpenProject.value = { source: 'modrinth', projectId: OCULUS_MODRINTH_PROJECT_ID };
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

  // i18n labels for the kind switch — order mirrors CONTENT_KINDS. Instance
  // Add-ons never surface the "plugin" ContentKind (plugins install to
  // servers, not client instances — see servers/mods/ServerModBrowser +
  // Task 16's plugin browser), so these maps exclude it explicitly.
  const kindLabels: Record<InstanceContentKind, TranslationKey> = {
    mod: 'addons.kindMods',
    resource_pack: 'addons.kindResourcePacks',
    shader: 'addons.kindShaders',
    datapack: 'addons.kindDatapacks',
  };
  const kindIcons: Record<InstanceContentKind, IconName> = {
    mod: 'blocks',
    resource_pack: 'resourcePack',
    shader: 'shader',
    datapack: 'datapack',
  };

  // Whether the active instance's Minecraft can load data packs at all (the
  // system arrived in 1.13). `true` with no instance selected — uncertainty
  // must not hide the feature; a genuine pre-1.13 answer hides the datapack
  // tab entirely (the whole feature is inert there: the game would read none
  // of it).
  //
  // Keyed on BOTH instance and mc version: the Manage modal changes an
  // instance's Minecraft IN PLACE (directory-as-identity — the id never
  // changes), so an id-only dependency would leave the gate stale after a
  // 1.21 → 1.12.2 downgrade with the tab still mounted.
  //
  // The module-level cache (below the component in module context) kills the
  // paint-then-vanish flash: AddonsTab remounts on every top-level tab
  // switch, and without a remembered answer a pre-1.13 instance would render
  // the Data packs tab for one frame on EVERY visit before the IPC answer
  // lands and yanks it. It also covers the reverse switch: landing on a new
  // instance seeds from ITS cached answer (or true), never the previous
  // instance's stale `false`.
  let supportsDatapacks = $state(true);
  $effect(() => {
    const id = instanceId;
    const mc = mcVersion;
    if (id === null) {
      supportsDatapacks = true;
      return;
    }
    const key = `${id}\n${mc ?? ''}`;
    supportsDatapacks = supportsDatapacksCache.get(key) ?? true;
    void (async () => {
      const r = await commands.instanceSupportsDatapacks(id);
      if (instanceId !== id || mcVersion !== mc) return;
      const v = r.status === 'ok' ? r.data : true;
      supportsDatapacksCache.set(key, v);
      supportsDatapacks = v;
    })();
  });

  // The datapack tab can vanish UNDER the active kind: the kind-reset effect
  // below deliberately reads only `kind`, so switching from a 1.21 instance
  // onto a 1.12.2 one would otherwise leave the datapack pane mounted with no
  // tab pointing at it (the spec's §7.1 second defect). Reset to 'mod', the
  // same default every other reset uses.
  $effect(() => {
    if (!supportsDatapacks && kind === 'datapack') kind = 'mod';
  });

  const kindOptions = $derived(
    CONTENT_KINDS.filter((k) => k !== 'datapack' || supportsDatapacks).map((k) => ({
      value: k,
      label: $t(kindLabels[k]),
      icon: kindIcons[k],
    })),
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
  // A status view requested by a deep-link (the Overview's incompatible-mods
  // indicator). Handed to the Installed view, which applies it once.
  let requestedFilter = $state<'incompatible' | null>(null);

  $effect(() => {
    if (modBrowserNav.value !== null) {
      view = modBrowserNav.value.view;
      if (modBrowserNav.value.view === 'installed') {
        installedOpenedForKind = new Set([...installedOpenedForKind, kind]);
        requestedFilter = modBrowserNav.value.filter ?? null;
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

  // Cross-view hand-off: the Installed view's pre-flight panel couldn't auto-
  // resolve a missing dependency, so jump to the Browse sub-tab with the dep id
  // pre-filled as the search query. ModBrowseView consumes the seed once and
  // calls back to clear it.
  let browseSeedQuery = $state<string | null>(null);
  function browseForDependency(query: string) {
    browseSeedQuery = query;
    selectView('browse');
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
    loaderVersion = null,
  }: {
    instanceId: string | null;
    instanceName?: string | null;
    mcVersion: string | null;
    loader: 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;
    loaderVersion?: string | null;
  } = $props();

  // Shader-loader detection. The installed-mods lookup runs only while the
  // Shaders segment is active and an instance is selected (guarded inside
  // refreshInstalledShaderMods); the mod-event listeners fire on any change but
  // no-op off the segment. Keeps the hint live so it disappears the moment a
  // working shader loader is installed. Purely informational, so a failed
  // lookup is swallowed (worst case the hint shows when it could have hidden).
  let installedShaderMods = $state<InstalledMod[]>([]);

  async function refreshInstalledShaderMods() {
    if (kind !== 'shader' || !instanceId) {
      installedShaderMods = [];
      return;
    }
    const reqId = instanceId;
    const r = await commands.modsListInstalled(reqId);
    if (instanceId !== reqId || r.status !== 'ok') return;
    installedShaderMods = r.data;
  }

  $effect(() => {
    // biome-ignore lint/correctness/noUnusedVariables: reactive read
    const _kind = kind;
    // biome-ignore lint/correctness/noUnusedVariables: reactive read
    const _id = instanceId;
    void refreshInstalledShaderMods();
  });

  // Race-safe registration/teardown (a fast tab flip can unmount this
  // component before listen() resolves) + burst coalescing: a with-deps
  // install emits one event per jar.
  const debouncedShaderModsRefresh = debounceTrailing(() => void refreshInstalledShaderMods(), 150);
  onDestroy(debouncedShaderModsRefresh.cancel);
  listenUntilDestroyed([
    events.modInstalled.listen(debouncedShaderModsRefresh.call),
    events.modUninstalled.listen(debouncedShaderModsRefresh.call),
    events.modToggle.listen(debouncedShaderModsRefresh.call),
  ]);

  // Shader loaders that work on this instance's loader, and which of them are
  // already installed. The banner shows only when none are.
  const shaderOptions = $derived(shaderLoaderOptions(loader));
  const detectedShaderLoaders = $derived(
    detectInstalledShaderLoaders(installedShaderMods, shaderOptions),
  );

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

  // Zips dropped on a Resource-pack/Shader/Datapack segment arrive via
  // droppedAssets (routed by MainTabs). Consume and reset; guard that the
  // payload kind still matches the active segment (the user may have switched
  // kinds mid-drag). Datapacks fork HERE, not in the router: they install
  // into the instance's library, never through `assetInstallLocal`, whose
  // `require_asset_kind` gate rejects the kind with a modpack-overrides
  // error message.
  $effect(() => {
    const v = droppedAssets.value;
    if (v !== null) {
      droppedAssets.value = null;
      if (v.kind !== kind || kind === 'mod') return;
      if (kind === 'datapack') void installDatapacksFromFiles(v.paths);
      else void installAssetsFromFiles(v.paths);
    }
  });

  async function installFromFile() {
    if (installDisabled) return;
    const r = await openFile({
      multiple: true,
      filters: [{ name: get(t)('common.fileFilter.modJar'), extensions: ['jar'] }],
    });
    if (Array.isArray(r) && r.length > 0) await onJarsPicked(r);
  }

  async function installAssetsFromPicker() {
    if (assetInstallDisabled) return;
    const r = await openFile({
      multiple: true,
      filters: [{ name: get(t)('common.fileFilter.addonZip'), extensions: ['zip'] }],
    });
    if (Array.isArray(r) && r.length > 0) await installAssetsFromFiles(r);
  }

  async function installDatapacksFromPicker() {
    if (assetInstallDisabled) return;
    const r = await openFile({
      multiple: true,
      filters: [{ name: get(t)('common.fileFilter.datapack'), extensions: ['zip'] }],
    });
    if (Array.isArray(r) && r.length > 0) await installDatapacksFromFiles(r);
  }

  // The world picker opened after a local datapack install — §7.3: it opens
  // after a successful install from the catalog, the file picker, AND once
  // per drag-drop batch. This is where the "installed means live" wrong
  // belief would otherwise form: the pack is in the library and the game
  // reads none of it until a world is picked.
  let datapackPickerTarget = $state<{
    filename: string;
    packName: string;
    placements: DatapackPlacementView[];
  } | null>(null);

  // Local datapack zips (file picker + drag-drop) go into the instance's
  // LIBRARY; the world picker then opens ONCE per batch, targeting the last
  // successful install (a multi-zip drop is rare, and each pack stays
  // reachable through the library screen's own «Add to worlds…»).
  async function installDatapacksFromFiles(paths: string[]) {
    if (instanceId === null) return;
    let ok = 0;
    let last: { filename: string; packName: string } | null = null;
    const failed: string[] = [];
    for (const path of paths) {
      const r = await commands.datapacksInstallFromFile(instanceId, path);
      if (r.status === 'ok') {
        ok += 1;
        last = { filename: r.data.filename, packName: r.data.name };
      } else {
        failed.push(`${filenameOf(path)}: ${formatError(r.error)}`);
      }
    }
    if (ok > 0) pushSuccess(get(t)('addons.install.toastInstalled', { count: ok }));
    if (failed.length > 0)
      pushWarning(get(t)('addons.install.toastFailed', { count: failed.length }), failed);
    // Refresh the Installed-datapacks view + Browse badges (no Tauri events).
    if (ok > 0) {
      datapacksChanged.value++;
      if (last !== null) {
        // Real placements, not [] — a reinstall over a pack already linked in
        // worlds must show each world's CURRENT state, or the picker would
        // route a present-but-disabled world through add (which re-enables)
        // instead of the explicit toggle.
        const lib = await commands.datapacksListLibrary(instanceId);
        const entry =
          lib.status === 'ok'
            ? lib.data.entries.find((e) => e.pack.filename === last.filename)
            : undefined;
        datapackPickerTarget = { ...last, placements: entry?.placements ?? [] };
      }
    }
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
    // The platform axis carries its own copy per axis: a loader-VERSION range
    // violation is not the loader-FAMILY case, and `mismatchLoader` ("this is
    // a Fabric jar") would read as nonsense for it.
    if (v.platform_mismatch && v.platform_declared) {
      parts.push(
        v.platform_axis === 'minecraft'
          ? translate('mods.browse.mismatchMc', {
              detected: v.platform_declared,
              instance: mcVersion ?? '',
            })
          : translate('mods.browse.mismatchPlatformLoader', {
              detected: v.platform_declared,
              instance: loaderVersion ?? '',
            }),
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
      if (v.loader_mismatch || v.platform_mismatch) {
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
  <div class="px-3 pt-1" data-tour-ctx="addons-kind-switch">
    <TabBar
      tabs={kindOptions.map((o) => ({
        id: o.value,
        label: o.label,
        icon: o.icon,
        // Rainbow-hover the Shaders icon only, gated by the same preference as
        // the sidebar Browse-modpacks icon. Empty string = no effect.
        iconClass: o.value === 'shader' && rainbowFx.enabled ? 'icon-rainbow-hover' : '',
      }))}
      active={kind}
      ariaLabel={$t('addons.kindSwitchAria')}
      testid="addons-kind-switch"
      onChange={(id) => (kind = id as InstanceContentKind)}
    />
  </div>

  <!-- Sub-tab row. Underline style — matches the Modpacks tab's
       Browse/Imported sub-tabs and the top-level tab row. -->
  <div
    class="flex items-center justify-between px-3 border-b border-border-subtle bg-surface mt-3"
    data-tour-ctx="addons-subtabs"
  >
    <TabBar
      tabs={[
        { id: 'browse', label: $t('mods.browse.tabBrowse') },
        { id: 'installed', label: $t('mods.browse.tabInstalled') },
      ]}
      active={view}
      ariaLabel={$t('addons.subTabsLabel')}
      onChange={(id) => selectView(id as View)}
    />
    <SourcePicker value={source} onChange={(v) => (source = v)} />
  </div>

  {#if kind === 'shader' && detectedShaderLoaders.length === 0}
    <!-- Non-blocking info banner: shaders need a shader loader to run. We list
         only the loaders that work on this instance's loader (all of them, not
         just one). Iris/Oculus open their mod detail modal in-app; OptiFine has
         no in-app install path, so it opens its downloads page. Hidden entirely
         once an applicable loader is detected as installed. -->
    <div class="px-3 pt-3">
      <div
        class="bg-accent/10 border border-accent/40 text-secondary text-sm rounded p-2 space-y-1.5"
        role="note"
      >
        <p>
          {shaderOptions.length === 1
            ? $t('addons.shaderLoaderHint.introOne')
            : $t('addons.shaderLoaderHint.intro')}
        </p>
        <p class="flex flex-wrap items-center gap-x-2 gap-y-1">
          {#each shaderOptions as id, i (id)}
            {#if i > 0}
              <span class="text-placeholder">{$t('addons.shaderLoaderHint.or')}</span>
            {/if}
            {#if id === 'optifine'}
              <button
                type="button"
                class="inline-flex items-center gap-1 font-medium text-accent underline underline-offset-2 hover:no-underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2 rounded"
                onclick={openOptifine}
              >
                {$t('addons.shaderLoaderHint.optifine')}<Icon name="externalLink" size={14} />
              </button>
            {:else}
              <button
                type="button"
                class="font-medium text-accent underline underline-offset-2 hover:no-underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2 rounded"
                onclick={id === 'iris' ? openIris : openOculus}
              >
                {id === 'iris'
                  ? $t('addons.shaderLoaderHint.iris')
                  : $t('addons.shaderLoaderHint.oculus')}
              </button>
            {/if}
          {/each}
        </p>
        {#if shaderOptions.includes('optifine')}
          <p class="text-xs text-muted">
            {#if mcVersion}
              {$t('addons.shaderLoaderHint.optifineVersion', { mc: mcVersion })}
            {:else}
              {$t('addons.shaderLoaderHint.optifineNoInstance')}
            {/if}
          </p>
        {/if}
      </div>
    </div>
  {/if}

  {#if kind === 'mod'}
    <div class="px-3 pt-3" data-tour-ctx="addons-dropzone">
      <FileDropzone
        label={$t('mods.browse.dropzoneLabel')}
        disabled={installDisabled}
        disabledLabel={$t('mods.browse.dropzoneDisabled')}
        onClick={installFromFile}
      />
    </div>
  {/if}

  {#if kind === 'resource_pack' || kind === 'shader' || kind === 'datapack'}
    <div class="px-3 pt-3" data-tour-ctx="addons-dropzone">
      <FileDropzone
        label={kind === 'resource_pack'
          ? $t('addons.dropzoneResourcePack')
          : kind === 'shader'
            ? $t('addons.dropzoneShader')
            : $t('addons.dropzoneDatapack')}
        disabled={assetInstallDisabled}
        disabledLabel={$t('addons.dropzoneDisabled')}
        onClick={kind === 'datapack' ? installDatapacksFromPicker : installAssetsFromPicker}
      />
    </div>
  {/if}

  <div class="flex-1 overflow-y-auto relative">
    <div class:hidden={view !== 'browse'}>
      <!-- Re-key per kind so switching content type resets the browse
           filters/results instead of leaking the previous kind's state. -->
      {#key kind}
        <ModBrowseView
          {kind}
          {source}
          {instanceId}
          {instanceName}
          {mcVersion}
          {loader}
          seedQuery={browseSeedQuery}
          onSeedConsumed={() => (browseSeedQuery = null)}
        />
      {/key}
    </div>
    {#if installedMounted}
      <div class:hidden={view !== 'installed'}>
        {#if kind === 'mod'}
          <InstalledModsView
            {instanceId}
            {mcVersion}
            {loader}
            {requestedFilter}
            onFilterApplied={() => (requestedFilter = null)}
            onBrowseFor={browseForDependency}
          />
        {:else if kind === 'datapack'}
          <!-- An explicit branch, not the assets fallback: a datapack's
               Installed view is the LIBRARY screen — per-world placements,
               cascade removal — and the assets view's commands reject the
               kind at the backend boundary anyway. -->
          <InstalledDatapacksView {instanceId} {mcVersion} {loader} />
        {:else}
          <InstalledAssetsView {instanceId} {kind} {mcVersion} {loader} />
        {/if}
      </div>
    {/if}
  </div>
</div>

{#if datapackPickerTarget && instanceId}
  <DatapackWorldPicker
    {instanceId}
    filename={datapackPickerTarget.filename}
    packName={datapackPickerTarget.packName}
    placements={datapackPickerTarget.placements}
    onClose={() => (datapackPickerTarget = null)}
    onApplied={() => {
      datapacksChanged.value++;
    }}
  />
{/if}

<ContextualTour id="addons" steps={ADDONS_STEPS} />

{#if mismatchRows.length > 0}
  <CompatWarningDialog
    rows={mismatchRows}
    busy={installingLocal}
    onConfirm={confirmInstallAll}
    onCancel={cancelMismatched}
  />
{/if}

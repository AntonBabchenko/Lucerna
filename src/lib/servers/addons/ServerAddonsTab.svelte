<script module lang="ts">
  // Session-lived answers for the 1.13 datapack gate, keyed by the Minecraft
  // version string — the command's ONLY input, so nothing else can be part of
  // the answer, and one entry serves every server on that version. Module-level
  // so it survives ServerAddonsTab remounting on every server switch.
  const supportsDatapacksCache = new Map<string, boolean>();
</script>

<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { get } from 'svelte/store';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { commands, type LevelDatPresence, type ModSource } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import TabBar from '$lib/ui/TabBar.svelte';
  import type { IconName } from '$lib/ui/icons';
  import SourcePicker from '$lib/mods/SourcePicker.svelte';
  import FileDropzone from '$lib/mods/FileDropzone.svelte';
  import { reportNotAdded } from '$lib/layout/drop-report';
  import { coreToLoaderKind } from '$lib/servers/core-display';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import {
    droppedServerContent,
    serverAddonsKind,
    type ServerAddonsKind,
  } from '$lib/settings/state.svelte';
  import ServerModBrowser from '$lib/servers/mods/ServerModBrowser.svelte';
  import ServerPluginBrowser from '$lib/servers/plugins/ServerPluginBrowser.svelte';
  import ServerDatapackBrowser from '$lib/servers/datapacks/ServerDatapackBrowser.svelte';
  import ServerDatapacksInstalled from '$lib/servers/datapacks/ServerDatapacksInstalled.svelte';
  import { serverWorldBlockedKey } from '$lib/servers/datapacks/datapack-rows';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { SERVER_ADDONS_STEPS } from '$lib/onboarding/contextual-tours';
  import ServerModsInstalled from './ServerModsInstalled.svelte';
  import ServerPluginsInstalled from './ServerPluginsInstalled.svelte';
  import { kindsFor } from './addon-kinds';

  // The server Add-ons tab: mirror of the client AddonsTab. Level 1 is the
  // content-kind switch (kinds gated by the server core AND the 1.13
  // datapack gate — see `kindsFor`/`supportsDatapacks` below), level 2 the
  // Browse/Installed sub-tabs, shared by all three kinds including datapacks.
  // The tab-level dropzone (visible in both sub-views, client parity) owns
  // ALL local-file installs.
  //
  // `visible` mirrors the prop ServersPanel itself takes from +page.svelte:
  // the whole servers panel stays MOUNTED (class:hidden) while the launcher is
  // in client mode, so the tour at the bottom of this file must be gated on it
  // — see the comment there.
  let { serverId, visible }: { serverId: string; visible: boolean } = $props();

  const server = $derived(serverState.list.find((s) => s.id === serverId) ?? null);
  const running = $derived(server?.running ?? false);
  const canMutate = $derived(server !== null && !running);

  // Whether this server's Minecraft can load data packs at all (the system
  // arrived in 1.13). `true` until the answer lands: uncertainty must not
  // hide the feature — the same rule `compat::supports_datapacks` encodes for
  // an unparseable version.
  //
  // The module-level cache kills the paint-then-vanish flash: this component
  // remounts on every server switch, and without a remembered answer a
  // pre-1.13 server would render the Datapacks tab for one frame on every
  // visit before the IPC answer lands and yanks it.
  let supportsDatapacks = $state(true);
  // Whether the gate ANSWER has landed (cache hit or IPC return) for the
  // current mc version. The tour mount at the bottom of this file must wait for
  // it: the optimistic `true` default makes `kinds` transiently non-empty on a
  // pre-1.13 vanilla server's first visit, and a tour that mounts on that one
  // frame is unmount-burned the moment the real answer empties `kinds`.
  let gateResolved = $state(false);
  $effect(() => {
    const mc = server?.mc_version ?? '';
    const cached = supportsDatapacksCache.get(mc);
    supportsDatapacks = cached ?? true;
    gateResolved = cached !== undefined;
    void (async () => {
      const v = await commands.mcVersionSupportsDatapacks(mc);
      // The user can switch servers while this is in flight.
      if ((server?.mc_version ?? '') !== mc) return;
      supportsDatapacksCache.set(mc, v);
      supportsDatapacks = v;
      gateResolved = true;
    })();
  });

  // Kinds this core + Minecraft version takes: mod loaders → mods(+datapacks
  // on 1.13+), paper/purpur → plugins(+datapacks on 1.13+), vanilla →
  // datapacks only on 1.13+. Can be EMPTY — a pre-1.13 vanilla server takes
  // none of the three; see the {#if kinds.length === 0} empty state below.
  const kinds = $derived(server ? kindsFor(server.loader, supportsDatapacks) : []);

  // Seeded with the first offered kind (client parity: the switch's first tab
  // is active on entry); `untrack` reads the initial value without subscribing.
  // Falls back to 'mod' when kinds is initially empty — a value that is never
  // rendered (the empty state takes over instead of the kind switch) and gets
  // corrected by the repair effect below the moment kinds becomes non-empty.
  let kind = $state<ServerAddonsKind>(untrack(() => kinds[0] ?? 'mod'));
  // Repair on core switch or a gate answer landing (paper→vanilla drops
  // 'plugin'; a 1.13+→pre-1.13 mc_version change drops 'datapack'). Guarded
  // on kinds.length: an empty kinds must not index kinds[0] (undefined).
  $effect(() => {
    if (kinds.length > 0 && !kinds.includes(kind)) kind = kinds[0];
  });

  type View = 'browse' | 'installed';
  let view = $state<View>('browse');
  let source = $state<ModSource>('modrinth');

  // Kind change resets the sub-view (client parity, prevKind-guarded).
  // `prevKind` is intentionally non-reactive and seeded with `untrack` so the
  // guard skips the first render while still firing on later kind changes.
  let prevKind = untrack(() => kind);
  $effect(() => {
    if (kind !== prevKind) {
      prevKind = kind;
      view = 'browse';
    }
  });

  // Mirror the active kind for the window drop router in +page.svelte; reset
  // on destroy so a stale kind never poisons a future drop.
  $effect(() => {
    serverAddonsKind.value = kind;
  });
  onDestroy(() => {
    serverAddonsKind.value = null;
  });

  // Installed panes re-read when this bumps (browser/dropzone installs).
  let reloadToken = $state(0);
  let dropError = $state<string | null>(null);
  // The Installed pane of the current kind is loaded and empty (it reports so): while that pane
  // shows, the strip gives way to its full drop area.
  let installedEmpty = $state(false);

  // The server world's level.dat presence, from the data pack Installed pane's
  // own read (bound below; that pane is mounted whenever the kind is
  // datapack). A world with only level.dat_old refuses every data pack change
  // until a server run restores level.dat (D2), so every add path here — the
  // zone's click, a window drop, the catalog — is off and says why, as the
  // pane's own controls are. Null (not read yet, or could not tell) is not a
  // verdict: the backend re-checks before writing.
  let datapackLevelDat = $state<LevelDatPresence | null>(null);
  const datapackBlock = $derived(
    kind === 'datapack' ? serverWorldBlockedKey(datapackLevelDat) : null,
  );
  // Why nothing can be added right now, or null: the running server first
  // (it owns the world), then the world itself.
  const addBlockedLabel = $derived(
    !canMutate
      ? $t('servers.mods.stopToManage')
      : datapackBlock !== null
        ? $t(datapackBlock)
        : null,
  );

  async function installLocalPaths(paths: string[]): Promise<void> {
    if (addBlockedLabel !== null) return;
    dropError = null;
    for (const p of paths) {
      const res =
        kind === 'mod'
          ? await commands.serverInstallLocal(serverId, p)
          : kind === 'plugin'
            ? await commands.serverInstallPluginLocal(serverId, p)
            : await commands.serverInstallDatapack(serverId, p);
      if (res.status === 'ok') {
        // Success toast: the exact key+params the legacy flows used —
        // servers.mods.localInstalled for jars, servers.mods.datapackInstalled
        // for datapacks (param `name`, per the retired Mods tab / ServerDatapacks).
        pushSuccess(
          get(t)(
            kind === 'datapack' ? 'servers.mods.datapackInstalled' : 'servers.mods.localInstalled',
            { name: String(res.data) },
          ),
        );
      } else {
        dropError = formatError(res.error);
        break;
      }
    }
    reloadToken++;
  }

  async function pickAndInstall(): Promise<void> {
    const filter =
      kind === 'mod'
        ? { name: get(t)('common.fileFilter.mod'), extensions: ['jar'] }
        : kind === 'plugin'
          ? { name: get(t)('common.fileFilter.pluginJar'), extensions: ['jar'] }
          : { name: get(t)('common.fileFilter.datapack'), extensions: ['zip'] };
    const picked = await openFile({ multiple: true, filters: [filter] });
    const paths = Array.isArray(picked) ? picked : typeof picked === 'string' ? [picked] : [];
    if (paths.length > 0) await installLocalPaths(paths);
  }

  // Drops routed here by +page.svelte (window drop router); consume only our kind. The router
  // cannot see the world's level.dat, so a drop can arrive while the drop zone is off: it is
  // refused here — and said so, in the zone's own words (a drop is never discarded in silence).
  $effect(() => {
    const payload = droppedServerContent.value;
    if (payload && payload.kind === kind) {
      droppedServerContent.value = null;
      const blocked = addBlockedLabel;
      if (blocked !== null) {
        reportNotAdded(payload.paths.map((path) => ({ path, reason: blocked })));
        return;
      }
      void installLocalPaths(payload.paths);
    }
  });

  // Per-kind icons, mirroring the client kindIcons map (blocks = client Mods
  // kind; plug/world are the server-only kinds).
  const KIND_ICONS: Record<ServerAddonsKind, IconName> = {
    mod: 'blocks',
    plugin: 'plug',
    datapack: 'datapack',
  };

  const kindOptions = $derived(
    kinds.map((k) => ({
      id: k,
      label:
        k === 'mod'
          ? $t('addons.kindMods')
          : k === 'plugin'
            ? $t('servers.addons.kindPlugins')
            : $t('servers.addons.kindDatapacks'),
      icon: KIND_ICONS[k],
    })),
  );
  const dropzoneLabel = $derived(
    kind === 'mod'
      ? $t('mods.browse.dropzoneLabel')
      : kind === 'plugin'
        ? $t('servers.addons.dropzonePlugin')
        : $t('servers.addons.dropzoneDatapack'),
  );
</script>

<!-- min-h-full: the drop area's box below grows to the panel's height (DESIGN.md §14). -->
<div class="flex flex-col gap-3 min-h-full">
  {#if kinds.length === 0}
    <!-- The hole the 1.13 gate opens: a pre-1.13 vanilla server is neither
         mod- nor plugin-capable, and datapacks are gated off too, so there is
         nothing this tab can offer. -->
    <p class="text-sm text-secondary" data-testid="server-addons-no-kinds">
      {$t('servers.addons.noKinds')}
    </p>
  {:else}
    <div data-tour-ctx="server-addons-kind-switch">
      <TabBar
        tabs={kindOptions}
        active={kind}
        ariaLabel={$t('addons.kindSwitchAria')}
        testid="server-addons-kind-switch"
        onChange={(id) => (kind = id as ServerAddonsKind)}
      />
    </div>

    <div class="flex items-center justify-between border-b border-border-subtle">
      <TabBar
        tabs={[
          { id: 'browse', label: $t('mods.browse.tabBrowse') },
          { id: 'installed', label: $t('mods.browse.tabInstalled') },
        ]}
        active={view}
        ariaLabel={$t('addons.subTabsLabel')}
        testid="server-addons-subtabs"
        onChange={(id) => (view = id as View)}
      />
      <!-- The host owns the picker (browsers render showSourcePicker={false});
           the plugin catalogue pairing is modrinth+hangar, same as the plugin
           browser's own inline picker; datapack falls through to `undefined`,
           the default Modrinth+CurseForge pairing. -->
      <SourcePicker
        value={source}
        onChange={(v) => (source = v)}
        options={kind === 'plugin' ? ['modrinth', 'hangar'] : undefined}
      />
    </div>

    <!-- The strip's drag overlay covers this box — the strip and the panes under it (DESIGN.md
         §14); it fills the tab's height, so the overlay does too. An empty Installed list holds
         the full drop area instead of the strip. -->
    <div class="relative flex-1 flex flex-col gap-3">
      {#if !(view === 'installed' && installedEmpty)}
        <div data-tour-ctx="server-addons-dropzone">
          <FileDropzone
            variant="strip"
            target="server-content"
            label={dropzoneLabel}
            disabled={addBlockedLabel !== null}
            disabledLabel={addBlockedLabel ?? undefined}
            dragLabel={$t('common.dropToAdd', { name: server?.name ?? '' })}
            onClick={() => void pickAndInstall()}
          />
        </div>
      {/if}
      {#if dropError}
        <p class="text-sm text-danger" role="alert">{dropError}</p>
      {/if}
      {#if running}
        <p class="text-xs text-warning-text">{$t('servers.mods.stopToManage')}</p>
      {/if}

      {#if server}
        <div class:hidden={view !== 'browse'}>
          <!-- Re-key per kind so switching content type resets filters/results. -->
          {#key kind}
            {#if kind === 'mod'}
              <!-- The ! is safe: 'mod' is only offered when modCapable, and
                 mod-capable cores are never paper/purpur (see core-display). -->
              <ServerModBrowser
                {serverId}
                mcVersion={server.mc_version}
                loader={coreToLoaderKind(server.loader)!}
                bind:source
                showSourcePicker={false}
                onInstalled={() => reloadToken++}
              />
            {:else if kind === 'plugin'}
              <ServerPluginBrowser
                {serverId}
                mcVersion={server.mc_version}
                core={server.loader}
                bind:source
                showSourcePicker={false}
                onInstalled={() => reloadToken++}
              />
            {:else}
              <ServerDatapackBrowser
                {serverId}
                mcVersion={server.mc_version}
                bind:source
                showSourcePicker={false}
                blockedReason={datapackBlock !== null ? $t(datapackBlock) : null}
                onInstalled={() => reloadToken++}
              />
            {/if}
          {/key}
        </div>
        <!-- Kept mounted (mirrors the Browse block above) so switching
           Browse↔Installed never remounts it: rows persist across visits, and
           the cold load runs in the background while the user is on Browse.
           `reloadToken` still refreshes it after a Browse/dropzone install. -->
        <div class:hidden={view !== 'installed'}>
          {#if kind === 'mod'}
            <ServerModsInstalled
              {serverId}
              {reloadToken}
              emptyDropzone={view === 'installed' ? installedDropzone : undefined}
              onEmptyChange={(e) => (installedEmpty = e)}
            />
          {:else if kind === 'plugin'}
            <ServerPluginsInstalled
              {serverId}
              {reloadToken}
              emptyDropzone={view === 'installed' ? installedDropzone : undefined}
              onEmptyChange={(e) => (installedEmpty = e)}
            />
          {:else}
            <ServerDatapacksInstalled
              {serverId}
              mcVersion={server.mc_version}
              disabled={running}
              {reloadToken}
              bind:levelDat={datapackLevelDat}
              emptyDropzone={view === 'installed' ? installedDropzone : undefined}
              onEmptyChange={(e) => (installedEmpty = e)}
            />
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

<!-- The empty Installed list's full drop area (DESIGN.md §14), handed over only while that list
     shows, so a hidden list never holds a second one. It carries the tour's anchor too. -->
{#snippet installedDropzone()}
  <div data-tour-ctx="server-addons-dropzone">
    <FileDropzone
      target="server-content"
      label={dropzoneLabel}
      disabled={addBlockedLabel !== null}
      disabledLabel={addBlockedLabel ?? undefined}
      onClick={() => void pickAndInstall()}
    />
  </div>
{/snippet}

<!-- Both steps anchor inside the {:else} branch above, so the tour may only
     mount once the branch is settled: `gateResolved` keeps it off the
     optimistic frame (see the gate effect), and `kinds.length` keeps it off the
     empty state, whose markup carries neither anchor.
     `visible` is the same rule ServersPanel applies to its own two tours: the
     servers panel stays mounted (display:none) in client mode, so without it a
     tour could fire — or keep running — with nothing on screen. Both halves
     matter, because the gate is REACTIVE: a mode switch while the tour is up
     unmounts it here, and ContextualTour's teardown hands the screen back
     (module claim + body flag together). Left active off-screen it would
     silently swallow every modal's Escape and every later tour's mount. -->
{#if visible && gateResolved && kinds.length > 0}
  <ContextualTour id="serverAddons" steps={SERVER_ADDONS_STEPS} />
{/if}

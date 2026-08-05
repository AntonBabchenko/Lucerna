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
  import { commands, type ModSource } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import TabBar from '$lib/ui/TabBar.svelte';
  import type { IconName } from '$lib/ui/icons';
  import SourcePicker from '$lib/mods/SourcePicker.svelte';
  import FileDropzone from '$lib/mods/FileDropzone.svelte';
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
  import ServerModsInstalled from './ServerModsInstalled.svelte';
  import ServerPluginsInstalled from './ServerPluginsInstalled.svelte';
  import { kindsFor } from './addon-kinds';

  // The server Add-ons tab: mirror of the client AddonsTab. Level 1 is the
  // content-kind switch (kinds gated by the server core AND the 1.13
  // datapack gate — see `kindsFor`/`supportsDatapacks` below), level 2 the
  // Browse/Installed sub-tabs, shared by all three kinds including datapacks.
  // The tab-level dropzone (visible in both sub-views, client parity) owns
  // ALL local-file installs.
  let { serverId }: { serverId: string } = $props();

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
  $effect(() => {
    const mc = server?.mc_version ?? '';
    supportsDatapacks = supportsDatapacksCache.get(mc) ?? true;
    void (async () => {
      const v = await commands.mcVersionSupportsDatapacks(mc);
      // The user can switch servers while this is in flight.
      if ((server?.mc_version ?? '') !== mc) return;
      supportsDatapacksCache.set(mc, v);
      supportsDatapacks = v;
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

  async function installLocalPaths(paths: string[]): Promise<void> {
    if (!canMutate) return;
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

  // Drops routed here by +page.svelte (window drop router); consume only our kind.
  $effect(() => {
    const payload = droppedServerContent.value;
    if (payload && payload.kind === kind) {
      droppedServerContent.value = null;
      void installLocalPaths(payload.paths);
    }
  });

  // Per-kind icons, mirroring the client kindIcons map (blocks = client Mods
  // kind; plug/world are the server-only kinds).
  const KIND_ICONS: Record<ServerAddonsKind, IconName> = {
    mod: 'blocks',
    plugin: 'plug',
    datapack: 'world',
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

<div class="flex flex-col gap-3">
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

    <div data-tour-ctx="server-addons-dropzone">
      <FileDropzone
        label={dropzoneLabel}
        disabled={!canMutate}
        disabledLabel={$t('servers.mods.stopToManage')}
        onClick={() => void pickAndInstall()}
      />
    </div>
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
          <ServerModsInstalled {serverId} {reloadToken} />
        {:else if kind === 'plugin'}
          <ServerPluginsInstalled {serverId} {reloadToken} />
        {:else}
          <ServerDatapacksInstalled
            {serverId}
            mcVersion={server.mc_version}
            disabled={running}
            {reloadToken}
          />
        {/if}
      </div>
    {/if}
  {/if}
</div>

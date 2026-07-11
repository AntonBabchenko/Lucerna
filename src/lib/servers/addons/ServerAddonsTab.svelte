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
  import { coreToLoaderKind, modCapable, pluginCapable } from '$lib/servers/core-display';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import {
    droppedServerContent,
    serverAddonsKind,
    type ServerAddonsKind,
  } from '$lib/settings/state.svelte';
  import ServerModBrowser from '$lib/servers/mods/ServerModBrowser.svelte';
  import ServerPluginBrowser from '$lib/servers/plugins/ServerPluginBrowser.svelte';
  import ServerDatapacks from '$lib/servers/mods/ServerDatapacks.svelte';
  import ServerModsInstalled from './ServerModsInstalled.svelte';
  import ServerPluginsInstalled from './ServerPluginsInstalled.svelte';

  // The server Add-ons tab: mirror of the client AddonsTab. Level 1 is the
  // content-kind switch (kinds gated by the server core), level 2 the
  // Browse/Installed sub-tabs; datapacks have no browse source, so that kind
  // is a flat installed view. The tab-level dropzone (visible in both
  // sub-views, client parity) owns ALL local-file installs.
  let { serverId }: { serverId: string } = $props();

  const server = $derived(serverState.list.find((s) => s.id === serverId) ?? null);
  const running = $derived(server?.running ?? false);
  const canMutate = $derived(server !== null && !running);

  // Kinds this core takes: mod loaders → mods+datapacks, paper/purpur →
  // plugins+datapacks, vanilla → datapacks only.
  const kinds = $derived.by(() => {
    const out: ServerAddonsKind[] = [];
    if (server && modCapable(server.loader)) out.push('mod');
    if (server && pluginCapable(server.loader)) out.push('plugin');
    out.push('datapack');
    return out;
  });

  // Seeded with the first offered kind (client parity: the switch's first tab
  // is active on entry); `untrack` reads the initial value without subscribing.
  let kind = $state<ServerAddonsKind>(untrack(() => kinds[0]));
  // Repair on core switch: paper→vanilla drops 'plugin' while it may be active.
  $effect(() => {
    if (!kinds.includes(kind)) kind = kinds[0];
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
        // for datapacks (param `name`, per ServerMods/ServerDatapacks).
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
  <div data-tour-ctx="server-addons-kind-switch">
    <TabBar
      tabs={kindOptions}
      active={kind}
      ariaLabel={$t('addons.kindSwitchAria')}
      testid="server-addons-kind-switch"
      onChange={(id) => (kind = id as ServerAddonsKind)}
    />
  </div>

  {#if kind !== 'datapack'}
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
           browser's own inline picker. -->
      <SourcePicker
        value={source}
        onChange={(v) => (source = v)}
        options={kind === 'plugin' ? ['modrinth', 'hangar'] : undefined}
      />
    </div>
  {/if}

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

  {#if kind === 'datapack'}
    <ServerDatapacks {serverId} disabled={running} />
  {:else if server}
    <div class:hidden={view !== 'browse'}>
      <!-- Re-key per kind so switching content type resets filters/results. -->
      {#key kind}
        {#if kind === 'mod'}
          <!-- The ! is safe: 'mod' is only offered when modCapable, and
               mod-capable cores are never paper/purpur (ServerMods parity). -->
          <ServerModBrowser
            {serverId}
            mcVersion={server.mc_version}
            loader={coreToLoaderKind(server.loader)!}
            bind:source
            showSourcePicker={false}
            onInstalled={() => reloadToken++}
          />
        {:else}
          <ServerPluginBrowser
            {serverId}
            mcVersion={server.mc_version}
            core={server.loader}
            bind:source
            showSourcePicker={false}
            onInstalled={() => reloadToken++}
          />
        {/if}
      {/key}
    </div>
    {#if view === 'installed'}
      {#if kind === 'mod'}
        <ServerModsInstalled {serverId} {reloadToken} />
      {:else}
        <ServerPluginsInstalled {serverId} {reloadToken} />
      {/if}
    {/if}
  {/if}
</div>

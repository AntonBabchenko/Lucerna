<script lang="ts">
  import { onMount } from 'svelte';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import MemorySlider from '$lib/instances/MemorySlider.svelte';
  import { formatHeapLabel } from '$lib/instances/heap';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Select, { type SelectOption } from '$lib/ui/Select.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { getProperty, setProperty } from '$lib/servers/properties-edit';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { serversUi } from '$lib/servers/servers-ui.svelte';
  import { displayCore, switchTargets } from '$lib/servers/core-display';
  import SwitchCoreModal from '$lib/servers/SwitchCoreModal.svelte';
  import DeleteServerDialog from '$lib/servers/DeleteServerDialog.svelte';
  import { SavedForm } from './saved-form.svelte';

  // The merged Settings tab: two independent save sections (launcher-side
  // launch config vs the server.properties file) because they persist through
  // different commands with different failure modes — one Save button would be
  // non-atomic. Core switch and the danger zone move in unchanged.
  let { serverId }: { serverId: string } = $props();

  const server = $derived(serverState.list.find((s) => s.id === serverId));
  const running = $derived(serverState.running(serverId));

  // ── Launch section (name / memory / JVM args) ──────────────────────────────
  let name = $state('');
  let memoryMb = $state(4096);
  let jvmArgs = $state('');
  let initialized = false;

  $effect(() => {
    const s = serverState.list.find((x) => x.id === serverId);
    if (s && !initialized) {
      initialized = true;
      name = s.name;
      memoryMb = s.max_heap_mb;
      jvmArgs = s.extra_jvm_args;
    }
  });

  let launchBusy = $state(false);
  let launchError = $state<string | null>(null);
  const launchSaved = new SavedForm();
  const canSaveLaunch = $derived(name.trim().length > 0);
  const launchSig = $derived(JSON.stringify({ name, memoryMb, jvmArgs }));
  $effect(() => launchSaved.sync(launchSig));

  async function saveLaunch() {
    if (!canSaveLaunch || launchBusy) return;
    launchBusy = true;
    launchError = null;
    try {
      const r1 = await serverState.rename(serverId, name.trim());
      if (!r1.ok) {
        launchError = formatError(r1.error as Parameters<typeof formatError>[0]);
        return;
      }
      const r2 = await serverState.updateRuntimeConfig(serverId, memoryMb, jvmArgs);
      if (!r2.ok) {
        launchError = formatError(r2.error as Parameters<typeof formatError>[0]);
        return;
      }
      launchSaved.markSaved(launchSig);
    } finally {
      launchBusy = false;
    }
  }

  // ── server.properties section ───────────────────────────────────────────────
  let raw = $state('');
  let loadError = $state<string | null>(null);
  let propsError = $state<string | null>(null);
  let propsBusy = $state(false);
  const propsSaved = new SavedForm();

  let port = $state('25565');
  let motd = $state('A Minecraft Server');
  let gamemode = $state('survival');
  let difficulty = $state('easy');
  let maxPlayers = $state('20');
  let onlineMode = $state(true);
  let pvp = $state(true);
  let whitelist = $state(false);

  const gamemodeOptions = $derived<SelectOption[]>([
    { value: 'survival', label: $t('servers.settings.gamemodeOptions.survival') },
    { value: 'creative', label: $t('servers.settings.gamemodeOptions.creative') },
    { value: 'adventure', label: $t('servers.settings.gamemodeOptions.adventure') },
    { value: 'spectator', label: $t('servers.settings.gamemodeOptions.spectator') },
  ]);
  const difficultyOptions = $derived<SelectOption[]>([
    { value: 'peaceful', label: $t('servers.settings.difficultyOptions.peaceful') },
    { value: 'easy', label: $t('servers.settings.difficultyOptions.easy') },
    { value: 'normal', label: $t('servers.settings.difficultyOptions.normal') },
    { value: 'hard', label: $t('servers.settings.difficultyOptions.hard') },
  ]);

  function syncFromRaw(text: string) {
    port = getProperty(text, 'server-port') ?? '25565';
    motd = getProperty(text, 'motd') ?? 'A Minecraft Server';
    gamemode = getProperty(text, 'gamemode') ?? 'survival';
    difficulty = getProperty(text, 'difficulty') ?? 'easy';
    maxPlayers = getProperty(text, 'max-players') ?? '20';
    onlineMode = (getProperty(text, 'online-mode') ?? 'true') !== 'false';
    pvp = (getProperty(text, 'pvp') ?? 'true') !== 'false';
    whitelist = (getProperty(text, 'white-list') ?? 'false') === 'true';
  }

  const propsSig = $derived(
    JSON.stringify({
      port,
      motd,
      gamemode,
      difficulty,
      maxPlayers,
      onlineMode,
      pvp,
      whitelist,
      raw,
    }),
  );
  $effect(() => propsSaved.sync(propsSig));

  // Tab bodies are {#if}-mounted by ServersPanel, so this runs on EVERY tab
  // activation — that is load-bearing: the Connect card (allow-offline), the
  // game itself, and server_start all rewrite server.properties, and a stale
  // `raw` snapshot would clobber their changes on the next save here.
  onMount(async () => {
    const res = await commands.serverReadProperties(serverId);
    if (res.status === 'ok') {
      raw = res.data;
      syncFromRaw(raw);
    } else {
      loadError = formatError(res.error);
    }
  });

  async function saveProps() {
    propsBusy = true;
    propsError = null;
    try {
      // Apply curated field edits on top of the current raw text so that any
      // advanced raw edits are preserved and curated fields win on conflicts
      // (last setProperty wins per key).
      let merged = raw;
      merged = setProperty(merged, 'server-port', port);
      merged = setProperty(merged, 'motd', motd);
      merged = setProperty(merged, 'gamemode', gamemode);
      merged = setProperty(merged, 'difficulty', difficulty);
      merged = setProperty(merged, 'max-players', maxPlayers);
      merged = setProperty(merged, 'online-mode', onlineMode ? 'true' : 'false');
      merged = setProperty(merged, 'pvp', pvp ? 'true' : 'false');
      merged = setProperty(merged, 'white-list', whitelist ? 'true' : 'false');

      const res = await commands.serverWriteProperties(serverId, merged);
      if (res.status === 'ok') {
        raw = merged;
        propsSaved.markSaved(propsSig);
      } else {
        propsError = formatError(res.error);
      }
    } finally {
      propsBusy = false;
    }
  }

  // ── Core switch + danger zone (moved in unchanged) ──────────────────────────
  let showSwitchCore = $state(false);
  let confirmingDelete = $state(false);
  let deleteError = $state<string | null>(null);
  let deleting = $state(false);

  async function confirmDelete() {
    confirmingDelete = false;
    deleteError = null;
    deleting = true;
    try {
      const r = await serverState.remove(serverId);
      if (!r.ok) {
        deleteError = formatError(r.error as Parameters<typeof formatError>[0]);
        return;
      }
      // Fall back to the first remaining server (or the empty state).
      serversUi.selectServer(serverState.list[0]?.id ?? null);
    } finally {
      deleting = false;
    }
  }
</script>

<div class="flex flex-col gap-6">
  <!-- Launch config -->
  <section class="flex flex-col gap-4">
    <h3 class="font-semibold">{$t('servers.settings.launchSection')}</h3>

    <div class="flex flex-col gap-1">
      <label for="sg-name" class="text-sm font-medium">{$t('servers.general.name')}</label>
      <input
        id="sg-name"
        type="text"
        maxlength="32"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={name}
      />
    </div>

    <div class="flex flex-col gap-1">
      <!-- svelte-ignore a11y_label_has_associated_control -->
      <label class="text-sm font-medium"
        >{$t('servers.general.memory')} · {formatHeapLabel(memoryMb)}</label
      >
      <MemorySlider valueMb={memoryMb} onInput={(mb) => (memoryMb = mb)} />
    </div>

    <div class="flex flex-col gap-1">
      <label for="sg-jvm" class="text-sm font-medium">{$t('servers.general.jvmArgs')}</label>
      <input
        id="sg-jvm"
        type="text"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 font-mono text-xs text-primary"
        bind:value={jvmArgs}
      />
      <p class="text-xs text-muted">{$t('servers.general.jvmArgsHint')}</p>
    </div>

    {#if running}
      <p class="text-xs text-warning-text">{$t('servers.general.restartToApply')}</p>
    {/if}

    <div class="flex items-center gap-3">
      <BusyButton
        class="btn-primary btn-sm"
        busy={launchBusy}
        disabled={!canSaveLaunch}
        data-testid="settings-launch-save"
        onclick={() => void saveLaunch()}
      >
        {$t('servers.general.save')}
      </BusyButton>
      {#if launchSaved.saved}
        <span class="text-xs text-success">{$t('servers.general.saved')}</span>
      {/if}
      {#if launchError}
        <span class="text-xs text-danger">{launchError}</span>
      {/if}
    </div>
  </section>

  <!-- server.properties -->
  <section class="flex flex-col gap-4 border-t border-border-subtle pt-4">
    <h3 class="font-semibold">{$t('servers.settings.propertiesSection')}</h3>

    {#if loadError}
      <p class="text-sm text-danger">{loadError}</p>
    {/if}

    <!-- Curated fields -->
    <div class="grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-3 text-sm">
      <!-- Port -->
      <label class="text-secondary whitespace-nowrap" for="sp-port">
        {$t('servers.settings.port')}
      </label>
      <input
        id="sp-port"
        type="number"
        min="1"
        max="65535"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={port}
      />

      <!-- MOTD -->
      <label class="text-secondary whitespace-nowrap" for="sp-motd">
        {$t('servers.settings.motd')}
      </label>
      <input
        id="sp-motd"
        type="text"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={motd}
      />

      <!-- Game mode -->
      <label class="text-secondary whitespace-nowrap" for="sp-gamemode">
        {$t('servers.settings.gamemode')}
      </label>
      <Select
        id="sp-gamemode"
        value={gamemode}
        options={gamemodeOptions}
        onChange={(v) => (gamemode = String(v))}
        ariaLabel={$t('servers.settings.gamemode')}
      />

      <!-- Difficulty -->
      <label class="text-secondary whitespace-nowrap" for="sp-difficulty">
        {$t('servers.settings.difficulty')}
      </label>
      <Select
        id="sp-difficulty"
        value={difficulty}
        options={difficultyOptions}
        onChange={(v) => (difficulty = String(v))}
        ariaLabel={$t('servers.settings.difficulty')}
      />

      <!-- Max players -->
      <label class="text-secondary whitespace-nowrap" for="sp-max-players">
        {$t('servers.settings.maxPlayers')}
      </label>
      <input
        id="sp-max-players"
        type="number"
        min="1"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={maxPlayers}
      />

      <!-- Online mode -->
      <label class="text-secondary whitespace-nowrap" for="sp-online-mode">
        {$t('servers.settings.onlineMode')}
      </label>
      <div class="flex items-center gap-2">
        <input
          id="sp-online-mode"
          type="checkbox"
          class="accent-accent cursor-pointer"
          bind:checked={onlineMode}
        />
      </div>

      <!-- PvP -->
      <label class="text-secondary whitespace-nowrap" for="sp-pvp">
        {$t('servers.settings.pvp')}
      </label>
      <div class="flex items-center gap-2">
        <input
          id="sp-pvp"
          type="checkbox"
          class="accent-accent cursor-pointer"
          bind:checked={pvp}
        />
      </div>

      <!-- Whitelist -->
      <label class="text-secondary whitespace-nowrap" for="sp-whitelist">
        {$t('servers.settings.whitelist')}
      </label>
      <div class="flex items-center gap-2">
        <input
          id="sp-whitelist"
          type="checkbox"
          class="accent-accent cursor-pointer"
          bind:checked={whitelist}
        />
      </div>
    </div>

    <!-- Advanced raw editor -->
    <details class="mt-2">
      <summary class="inline-flex items-center cursor-pointer text-sm text-secondary select-none">
        <span class="disclosure-caret mr-1"><Icon name="caret" size={14} /></span>
        {$t('servers.settings.raw')}
      </summary>
      <textarea
        class="mt-2 w-full rounded border border-border-emphasis bg-base px-2 py-1 font-mono text-xs text-primary"
        rows="12"
        bind:value={raw}
      ></textarea>
    </details>

    {#if running}
      <p class="text-xs text-warning-text">{$t('servers.settings.restartToApply')}</p>
    {/if}

    <div class="flex items-center gap-3">
      <BusyButton
        class="btn-primary btn-sm"
        busy={propsBusy}
        data-testid="settings-properties-save"
        onclick={() => void saveProps()}
      >
        {$t('servers.settings.save')}
      </BusyButton>
      {#if propsSaved.saved}
        <span class="text-xs text-success">{$t('servers.settings.saved')}</span>
      {/if}
      {#if propsError}
        <span class="text-xs text-danger">{propsError}</span>
      {/if}
    </div>
  </section>

  {#if server}
    <!-- Server core -->
    <section class="flex flex-col gap-2 border-t border-border-subtle pt-4">
      <h3 class="font-semibold mb-1">{$t('servers.core.sectionTitle')}</h3>
      <p class="text-sm text-secondary">
        {$t('servers.core.current')}: <span class="font-medium">{displayCore(server.loader)}</span>
        {#if server.loader_version}
          <span class="text-muted">({server.loader_version})</span>
        {/if}
      </p>
      {#if switchTargets(server.loader).length > 0}
        <div>
          <button
            type="button"
            class="btn-secondary btn-sm"
            disabled={server.running}
            onclick={() => (showSwitchCore = true)}
          >
            {$t('servers.core.switchButton')}
          </button>
          {#if server.running}
            <p class="text-xs text-muted mt-1">{$t('servers.core.stopToSwitch')}</p>
          {/if}
        </div>
      {/if}
    </section>
  {/if}

  <div class="border-t border-border-subtle pt-4 flex flex-col gap-2">
    <span class="text-sm font-medium text-danger">{$t('servers.general.dangerTitle')}</span>
    <span
      class="inline-flex self-start"
      use:tooltip={{
        text: running ? $t('servers.delete.runningBlock') : $t('servers.delete.trigger'),
        describe: false,
      }}
    >
      <button
        type="button"
        class="btn-danger btn-sm flex items-center gap-1.5"
        disabled={running || deleting}
        data-testid="server-delete-trigger"
        onclick={() => (confirmingDelete = true)}
      >
        <Icon name="trash" size={14} />
        {$t('servers.delete.trigger')}
      </button>
    </span>
    {#if deleteError}
      <span class="text-xs text-danger" role="alert">{deleteError}</span>
    {/if}
  </div>
</div>

{#if showSwitchCore && server}
  <SwitchCoreModal
    serverId={server.id}
    currentCore={server.loader}
    onClose={() => (showSwitchCore = false)}
  />
{/if}

{#if confirmingDelete}
  {@const s = serverState.list.find((x) => x.id === serverId)}
  <DeleteServerDialog
    serverName={s?.name ?? serverId}
    onCancel={() => (confirmingDelete = false)}
    onConfirm={() => void confirmDelete()}
  />
{/if}

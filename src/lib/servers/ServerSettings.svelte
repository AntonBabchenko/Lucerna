<script lang="ts">
  import { onMount } from 'svelte';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Select, { type SelectOption } from '$lib/ui/Select.svelte';
  import { Icon } from '$lib/ui/icons';
  import { getProperty, setProperty } from './properties-edit';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { displayCore, switchTargets } from '$lib/servers/core-display';
  import SwitchCoreModal from './SwitchCoreModal.svelte';

  let { serverId }: { serverId: string } = $props();

  const server = $derived(serverState.list.find((s) => s.id === serverId));
  let showSwitchCore = $state(false);

  // ── raw file state ──────────────────────────────────────────────────────────
  let raw = $state('');
  let loadError = $state<string | null>(null);
  let saveError = $state<string | null>(null);
  let saved = $state(false);
  let busy = $state(false);

  // ── curated field state ──────────────────────────────────────────────────────
  let port = $state('25565');
  let motd = $state('A Minecraft Server');
  let gamemode = $state('survival');
  let difficulty = $state('easy');
  let maxPlayers = $state('20');
  let onlineMode = $state(true);
  let pvp = $state(true);
  let whitelist = $state(false);

  // ── option lists ─────────────────────────────────────────────────────────────
  // Derived off `$t` so the option labels re-render on a live locale switch;
  // the `value`s stay the raw server.properties tokens the file expects.
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

  // Dirty guard (#34): drop the "Saved" pill the instant the user edits any
  // curated field or the raw text again, so it never lingers as a stale claim.
  const formSig = $derived(
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
  let savedSnapshot = $state<string | null>(null);
  $effect(() => {
    if (saved && formSig !== savedSnapshot) saved = false;
  });

  onMount(async () => {
    const res = await commands.serverReadProperties(serverId);
    if (res.status === 'ok') {
      raw = res.data;
      syncFromRaw(raw);
    } else {
      loadError = formatError(res.error);
    }
  });

  async function save() {
    busy = true;
    saveError = null;
    saved = false;
    try {
      // Apply curated field edits on top of the current raw text so that
      // any advanced raw edits are preserved and curated fields win on
      // conflicts (last setProperty wins per key).
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
        savedSnapshot = formSig;
        saved = true;
      } else {
        saveError = formatError(res.error);
      }
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex flex-col gap-4">
  {#if loadError}
    <p class="text-sm text-danger">{loadError}</p>
  {/if}

  {#if server}
    <!-- Server core -->
    <div class="flex flex-col gap-2">
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
    </div>
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
      <input id="sp-pvp" type="checkbox" class="accent-accent cursor-pointer" bind:checked={pvp} />
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

  <!-- Save row -->
  <div class="flex items-center gap-3">
    <BusyButton class="btn-primary btn-sm" {busy} onclick={() => void save()}>
      {$t('servers.settings.save')}
    </BusyButton>
    {#if saved}
      <span class="text-xs text-success">{$t('servers.settings.saved')}</span>
    {/if}
    {#if saveError}
      <span class="text-xs text-danger">{saveError}</span>
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

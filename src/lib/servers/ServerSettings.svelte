<script lang="ts">
  import { onMount } from 'svelte';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Select, { type SelectOption } from '$lib/ui/Select.svelte';
  import { getProperty, setProperty } from './properties-edit';

  let { serverId }: { serverId: string } = $props();

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
  const gamemodeOptions: SelectOption[] = [
    { value: 'survival', label: 'Survival' },
    { value: 'creative', label: 'Creative' },
    { value: 'adventure', label: 'Adventure' },
    { value: 'spectator', label: 'Spectator' },
  ];
  const difficultyOptions: SelectOption[] = [
    { value: 'peaceful', label: 'Peaceful' },
    { value: 'easy', label: 'Easy' },
    { value: 'normal', label: 'Normal' },
    { value: 'hard', label: 'Hard' },
  ];

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
      class="h-8 rounded border border-border-emphasis bg-surface px-2 text-primary"
      bind:value={port}
    />

    <!-- MOTD -->
    <label class="text-secondary whitespace-nowrap" for="sp-motd">
      {$t('servers.settings.motd')}
    </label>
    <input
      id="sp-motd"
      type="text"
      class="h-8 rounded border border-border-emphasis bg-surface px-2 text-primary"
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
      class="h-8 rounded border border-border-emphasis bg-surface px-2 text-primary"
      bind:value={maxPlayers}
    />

    <!-- Online mode -->
    <span class="text-secondary whitespace-nowrap">
      {$t('servers.settings.onlineMode')}
    </span>
    <label class="flex items-center gap-2 cursor-pointer">
      <input type="checkbox" class="accent-accent" bind:checked={onlineMode} />
    </label>

    <!-- PvP -->
    <span class="text-secondary whitespace-nowrap">
      {$t('servers.settings.pvp')}
    </span>
    <label class="flex items-center gap-2 cursor-pointer">
      <input type="checkbox" class="accent-accent" bind:checked={pvp} />
    </label>

    <!-- Whitelist -->
    <span class="text-secondary whitespace-nowrap">
      {$t('servers.settings.whitelist')}
    </span>
    <label class="flex items-center gap-2 cursor-pointer">
      <input type="checkbox" class="accent-accent" bind:checked={whitelist} />
    </label>
  </div>

  <!-- Advanced raw editor -->
  <details class="mt-2">
    <summary class="cursor-pointer text-sm text-secondary select-none">
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

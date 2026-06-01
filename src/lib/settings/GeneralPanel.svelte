<script lang="ts">
  // Settings → General. Appearance + playing-time preferences +
  // onboarding replay. Future general settings (language, update
  // prefs) accumulate here per the post-v0.5.0 backlog.
  import { onMount } from 'svelte';
  import { commands, type GeneralSettings, type ThemePreference } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { replayTour } from '$lib/onboarding/state.svelte';
  import { themeState, setThemePref } from '$lib/theme/state.svelte';
  import { runUpdate, updateInstalling, updateState } from '$lib/update/state.svelte';
  import { settingsOpen } from './state.svelte';

  let general = $state<GeneralSettings>({
    hide_to_tray_during_game: false,
    theme: 'system',
    check_updates_on_startup: true,
  });
  let loadError = $state<string | null>(null);
  let saveError = $state<string | null>(null);

  onMount(async () => {
    const r = await commands.appSettingsGet();
    if (r.status === 'ok') {
      general = r.data.general;
    } else {
      loadError = formatError(r.error);
    }
  });

  async function save() {
    saveError = null;
    const r = await commands.appSettingsSetGeneral(general);
    if (r.status !== 'ok') {
      saveError = formatError(r.error);
    }
  }

  // Manual update check, triggered from the button below. Mirrors the
  // startup check but reports inline rather than via the passive toast.
  type CheckResult =
    | { kind: 'idle' }
    | { kind: 'uptodate'; current: string }
    | { kind: 'available'; version: string }
    | { kind: 'error'; message: string };
  let checking = $state(false);
  let checkResult = $state<CheckResult>({ kind: 'idle' });

  async function checkForUpdates() {
    checking = true;
    checkResult = { kind: 'idle' };
    const r = await commands.updateCheck();
    checking = false;
    if (r.status !== 'ok') {
      checkResult = { kind: 'error', message: formatError(r.error) };
      return;
    }
    if (r.data.available) {
      updateState.value = r.data;
      checkResult = { kind: 'available', version: r.data.latest };
    } else {
      checkResult = { kind: 'uptodate', current: r.data.current };
    }
  }

  function onReplay() {
    replayTour();
    settingsOpen.value = null;
  }
</script>

<section class="flex flex-col gap-6">
  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm text-primary">Appearance</h3>
    <fieldset class="flex flex-col gap-2">
      {#each [{ v: 'system' as ThemePreference, label: 'System' }, { v: 'light' as ThemePreference, label: 'Light' }, { v: 'dark' as ThemePreference, label: 'Dark' }] as opt (opt.v)}
        <label class="flex items-center gap-2 cursor-pointer">
          <input
            type="radio"
            name="theme"
            value={opt.v}
            checked={themeState.pref === opt.v}
            onchange={() => void setThemePref(opt.v)}
            data-testid="theme-{opt.v}"
          />
          <span class="text-sm">{opt.label}</span>
        </label>
      {/each}
    </fieldset>
  </div>

  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm text-primary">Playing</h3>
    {#if loadError}
      <p class="text-xs text-danger">{loadError}</p>
    {/if}
    <label class="flex items-start gap-2 cursor-pointer">
      <input
        type="checkbox"
        class="mt-0.5"
        bind:checked={general.hide_to_tray_during_game}
        onchange={() => void save()}
        data-testid="tray-toggle"
      />
      <span class="flex-1">
        <span class="text-sm text-primary">Hide launcher to tray when Minecraft starts</span>
        <span class="block text-xs text-muted">
          A small icon appears in the system tray. The launcher returns when Minecraft closes; click
          the tray icon to bring it back sooner.
        </span>
      </span>
    </label>
    {#if saveError}
      <p class="text-xs text-danger">{saveError}</p>
    {/if}
  </div>

  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm text-primary">Updates</h3>
    <label class="flex items-start gap-2 cursor-pointer">
      <input
        type="checkbox"
        class="mt-0.5"
        bind:checked={general.check_updates_on_startup}
        onchange={() => void save()}
        data-testid="updates-toggle"
      />
      <span class="flex-1">
        <span class="text-sm text-primary">Check for updates on startup</span>
        <span class="block text-xs text-muted">
          Shows a notification when a newer version is available. Installing is always an explicit
          click — Lucerna never updates itself silently.
        </span>
      </span>
    </label>
    <div class="flex items-center gap-3 flex-wrap">
      <button
        type="button"
        class="btn-secondary btn-sm"
        onclick={() => void checkForUpdates()}
        disabled={checking}
        data-testid="check-updates-btn"
      >
        {checking ? 'Checking…' : 'Check for updates'}
      </button>
      {#if checkResult.kind === 'uptodate'}
        <p class="text-xs text-muted" data-testid="update-status">
          You're on the latest version ({checkResult.current}).
        </p>
      {:else if checkResult.kind === 'error'}
        <p class="text-xs text-danger" data-testid="update-status">
          Couldn't check: {checkResult.message}
        </p>
      {:else if checkResult.kind === 'available'}
        <p class="text-xs text-primary" data-testid="update-status">
          Version {checkResult.version} is available.
        </p>
        <button
          type="button"
          class="btn-primary btn-sm"
          onclick={() => void runUpdate()}
          disabled={updateInstalling.value}
          data-testid="update-now-btn"
        >
          {updateInstalling.value ? 'Installing…' : 'Update now'}
        </button>
      {/if}
    </div>
  </div>

  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm text-primary">Onboarding</h3>
    <div class="flex items-center gap-3">
      <button type="button" class="btn-secondary btn-sm" onclick={onReplay}>
        Replay onboarding tour
      </button>
      <p class="text-xs text-muted">Show the 6-step tutorial again.</p>
    </div>
  </div>
</section>

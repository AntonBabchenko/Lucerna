<script lang="ts">
  // Settings → General. Onboarding replay + the playing-time
  // preferences. Future general settings (theme, language, update
  // prefs) accumulate here per the post-v0.5.0 backlog.
  import { onMount } from 'svelte';
  import { commands, type GeneralSettings } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { replayTour } from '$lib/onboarding/state.svelte';
  import { settingsOpen } from './state.svelte';

  let general = $state<GeneralSettings>({ hide_to_tray_during_game: false });
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

  function onReplay() {
    replayTour();
    settingsOpen.value = null;
  }
</script>

<section class="flex flex-col gap-6">
  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm">Playing</h3>
    {#if loadError}
      <p class="text-xs text-red-700">{loadError}</p>
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
        <span class="text-sm text-neutral-800">Hide launcher to tray when Minecraft starts</span>
        <span class="block text-xs text-neutral-500">
          A small icon appears in the system tray. The launcher returns when Minecraft closes; click
          the tray icon to bring it back sooner.
        </span>
      </span>
    </label>
    {#if saveError}
      <p class="text-xs text-red-700">{saveError}</p>
    {/if}
  </div>

  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm">Onboarding</h3>
    <div class="flex items-center gap-3">
      <button type="button" class="btn-secondary btn-sm" onclick={onReplay}>
        Replay onboarding tour
      </button>
      <p class="text-xs text-neutral-500">Show the 6-step tutorial again.</p>
    </div>
  </div>
</section>

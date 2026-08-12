<script lang="ts">
  // Settings → Game. Hide-to-tray-during-game, preferred GPU, and the
  // saved-server status permission. Owns only those three GeneralSettings
  // fields; persists via a fresh read-modify-write that merges nothing else,
  // so it never clobbers a sibling panel's field.
  import { onMount } from 'svelte';
  import {
    commands,
    type GeneralSettings,
    type GpuCapability,
    type GpuPreference,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import Select from '$lib/ui/Select.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import SettingsField from './SettingsField.svelte';

  let gpuCap = $state<GpuCapability | null>(null);
  const gpuOptions = $derived<{ value: GpuPreference; label: string }[]>(
    gpuCap?.kind === 'available'
      ? [
          { value: 'auto', label: $t('settings.general.gpu.auto') },
          {
            value: 'high_performance',
            label: gpuCap.high
              ? `${$t('settings.general.gpu.high')} (${gpuCap.high})`
              : $t('settings.general.gpu.high'),
          },
          {
            value: 'power_saving',
            label: gpuCap.low
              ? `${$t('settings.general.gpu.power')} (${gpuCap.low})`
              : $t('settings.general.gpu.power'),
          },
        ]
      : [],
  );

  let general = $state<GeneralSettings>({
    hide_to_tray_during_game: false,
    theme: 'system',
    check_updates_on_startup: true,
    gpu_preference: 'auto',
    allow_server_ping: false,
  });
  let loadError = $state<string | null>(null);
  let saveError = $state<string | null>(null);

  let gpuLoading = $state(false);

  onMount(async () => {
    const r = await commands.appSettingsGet();
    if (r.status === 'ok') general = r.data.general;
    else loadError = formatError(r.error);
    try {
      gpuLoading = true;
      const c = await commands.gpuCapability();
      if (c.status === 'ok') gpuCap = c.data;
    } finally {
      gpuLoading = false;
    }
  });

  async function save() {
    saveError = null;
    // Snapshot our owned fields immediately (before any awaits) so that
    // async resolution of onMount or other concurrent callers cannot
    // overwrite `general` between snapshot and write.
    const tray = general.hide_to_tray_during_game;
    const gpu = general.gpu_preference;
    const ping = general.allow_server_ping;
    const cur = await commands.appSettingsGet();
    if (cur.status !== 'ok') {
      saveError = formatError(cur.error);
      return;
    }
    const next = {
      ...cur.data.general,
      hide_to_tray_during_game: tray,
      gpu_preference: gpu,
      allow_server_ping: ping,
    };
    const r = await commands.appSettingsSetGeneral(next);
    if (r.status !== 'ok') saveError = formatError(r.error);
  }
</script>

<section class="flex flex-col gap-6">
  <SettingsField anchor="game.tray">
    <div class="flex flex-col gap-3">
      <h3 class="font-medium text-sm text-primary">{$t('settings.general.playing.title')}</h3>
      {#if loadError}
        <p class="text-xs text-danger">{loadError}</p>
      {/if}
      <label class="flex items-start gap-2 cursor-pointer">
        <input
          type="checkbox"
          class="mt-0.5"
          checked={general.hide_to_tray_during_game}
          onchange={(e) => {
            general.hide_to_tray_during_game = e.currentTarget.checked;
            void save();
          }}
          data-testid="tray-toggle"
        />
        <span class="flex-1">
          <span class="text-sm text-primary">{$t('settings.general.playing.trayLabel')}</span>
          <span class="block text-xs text-muted">
            {$t('settings.general.playing.trayDescription')}
          </span>
        </span>
      </label>
      {#if saveError}
        <p class="text-xs text-danger">{saveError}</p>
      {/if}
    </div>
  </SettingsField>

  <!-- The consent gate for the whole server-status feature. Default off, and
       the IP-exposure consequence is stated right here rather than left for the
       user to infer — this is the one place they decide. -->
  <SettingsField anchor="game.serverPing">
    <div class="flex flex-col gap-3">
      <h3 class="font-medium text-sm text-primary">{$t('settings.general.serverPing.title')}</h3>
      <label class="flex items-start gap-2 cursor-pointer">
        <input
          type="checkbox"
          class="mt-0.5"
          checked={general.allow_server_ping}
          onchange={(e) => {
            general.allow_server_ping = e.currentTarget.checked;
            void save();
          }}
          data-testid="server-ping-toggle"
        />
        <span class="flex-1">
          <span class="text-sm text-primary">{$t('settings.general.serverPing.label')}</span>
          <span class="block text-xs text-muted">
            {$t('settings.general.serverPing.description')}
          </span>
          <span class="block text-xs text-warning-text">
            {$t('settings.general.serverPing.privacy')}
          </span>
        </span>
      </label>
    </div>
  </SettingsField>

  {#if gpuLoading}
    <LoadingPanel label={$t('common.loading')} size="sm" />
  {:else if gpuCap?.kind === 'available'}
    <SettingsField anchor="game.gpu">
      <div class="flex flex-col gap-3">
        <h3 class="font-medium text-sm text-primary">{$t('settings.general.gpu.title')}</h3>
        <div class="flex flex-col gap-1">
          <span class="text-sm text-primary">{$t('settings.general.gpu.label')}</span>
          <Select
            class="text-sm"
            dataTestid="gpu-select"
            ariaLabel={$t('settings.general.gpu.label')}
            value={general.gpu_preference ?? null}
            options={gpuOptions}
            onChange={(v) => {
              general.gpu_preference = v as GpuPreference;
              void save();
            }}
          />
          <span class="text-xs text-muted">{$t('settings.general.gpu.note')}</span>
        </div>
      </div>
    </SettingsField>
  {/if}
</section>

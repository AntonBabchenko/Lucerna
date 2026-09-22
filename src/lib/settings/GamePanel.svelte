<script lang="ts">
  // Settings → Game. Hide-to-tray-during-game, preferred GPU, and the
  // saved-server status permission. Owns only those three GeneralSettings
  // fields; persists via a fresh read-modify-write that merges nothing else,
  // so it never clobbers a sibling panel's field.
  import { onMount } from 'svelte';
  import {
    commands,
    type GeneralSettings,
    type GpuPreference,
    type GpuStatus,
  } from '$lib/ipc/bindings';
  import { describeStoreError, formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import Select from '$lib/ui/Select.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import SettingsField from './SettingsField.svelte';

  // The GPU block: checking → the OS answer (or the IPC failure). It never
  // claims "not available" before it has asked, and never folds "could not
  // tell" into "one GPU" — the backend keeps those apart, the page keeps them
  // apart.
  type GpuState = 'checking' | GpuStatus | { kind: 'ipc_error'; details: string };
  let gpu = $state<GpuState>('checking');
  const gpuCap = $derived(gpu !== 'checking' && 'capability' in gpu ? gpu.capability : null);
  const gpuMechanism = $derived(gpu !== 'checking' && 'mechanism' in gpu ? gpu.mechanism : null);
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
  // The note is true for the machine it is shown on: the backend names the
  // mechanism, and the page has no platform helper of its own.
  const gpuNote = $derived(
    $t(
      gpuMechanism === 'linux_env'
        ? 'settings.general.gpu.noteLinux'
        : 'settings.general.gpu.noteWindows',
    ),
  );
  const GPU_REASON: Record<'single_gpu' | 'unsupported', TranslationKey> = {
    single_gpu: 'settings.general.gpu.reason.singleGpu',
    unsupported: 'settings.general.gpu.reason.unsupported',
  };
  const gpuReason = $derived.by((): string | null => {
    if (gpu === 'checking') return null;
    if (!('capability' in gpu)) {
      return $t('settings.general.gpu.reason.unknown', { details: gpu.details });
    }
    const cap = gpu.capability;
    if (cap.kind === 'available') return null;
    if (cap.kind === 'unknown') {
      return $t('settings.general.gpu.reason.unknown', { details: cap.details });
    }
    return $t(GPU_REASON[cap.kind]);
  });

  let general = $state<GeneralSettings>({
    hide_to_tray_during_game: false,
    theme: 'system',
    check_updates_on_startup: true,
    gpu_preference: 'auto',
    allow_server_ping: false,
  });
  let loadError = $state<string | null>(null);
  let saveError = $state<string | null>(null);

  // A stored non-Auto choice keeps acting (where the OS lets it) while the
  // control is hidden; say so, per mechanism, and offer the way back. Nothing
  // is claimed while checking, on Automatic, or without a loaded `general`.
  const storedLabel = $derived(
    $t(
      general.gpu_preference === 'high_performance'
        ? 'settings.general.gpu.high'
        : 'settings.general.gpu.power',
    ),
  );
  const storedChoice = $derived.by((): string | null => {
    if (gpu === 'checking' || loadError !== null || general.gpu_preference === 'auto') return null;
    if (!('capability' in gpu)) {
      return $t('settings.general.gpu.stored.saved', { label: storedLabel });
    }
    if (gpu.capability.kind === 'available') return null;
    const acts =
      gpu.mechanism === 'windows_registry' ||
      (gpu.mechanism === 'linux_env' && general.gpu_preference === 'high_performance');
    if (!acts) return $t('settings.general.gpu.stored.inert', { label: storedLabel });
    return $t(
      gpu.mechanism === 'windows_registry'
        ? 'settings.general.gpu.stored.windows'
        : 'settings.general.gpu.stored.linux',
      { label: storedLabel },
    );
  });

  async function loadSettings() {
    const r = await commands.appSettingsGet();
    if (r.status === 'ok') general = r.data.general;
    else loadError = formatError(r.error);
  }
  async function loadGpu() {
    try {
      const c = await commands.gpuCapability();
      gpu = c.status === 'ok' ? c.data : { kind: 'ipc_error', details: formatError(c.error) };
    } catch (e) {
      // typedError rethrows real Error instances: a rejection is "could not tell".
      gpu = { kind: 'ipc_error', details: describeStoreError(e) };
    }
  }
  onMount(() => {
    void loadSettings();
    void loadGpu();
  });

  async function save() {
    saveError = null;
    // Snapshot our owned fields immediately (before any awaits) so that
    // async resolution of onMount or other concurrent callers cannot
    // overwrite `general` between snapshot and write.
    const tray = general.hide_to_tray_during_game;
    const gpuPref = general.gpu_preference;
    const ping = general.allow_server_ping;
    const cur = await commands.appSettingsGet();
    if (cur.status !== 'ok') {
      saveError = formatError(cur.error);
      return;
    }
    const next = {
      ...cur.data.general,
      hide_to_tray_during_game: tray,
      gpu_preference: gpuPref,
      allow_server_ping: ping,
    };
    const r = await commands.appSettingsSetGeneral(next);
    if (r.status !== 'ok') saveError = formatError(r.error);
  }
  function resetGpu() {
    general.gpu_preference = 'auto';
    void save();
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

  {#if gpu === 'checking'}
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
          <span class="text-xs text-muted" data-testid="gpu-note">{gpuNote}</span>
        </div>
      </div>
    </SettingsField>
  {:else}
    <SettingsField anchor="game.gpu">
      <div class="flex flex-col gap-2">
        <h3 class="font-medium text-sm text-primary">{$t('settings.general.gpu.title')}</h3>
        <span class="text-xs text-muted" data-testid="gpu-reason">{gpuReason}</span>
        {#if storedChoice}
          <p class="text-xs text-primary" data-testid="gpu-stored">{storedChoice}</p>
          <button
            type="button"
            class="btn-secondary btn-sm self-start"
            data-testid="gpu-reset"
            onclick={resetGpu}
          >
            {$t('settings.general.gpu.resetBtn')}
          </button>
        {/if}
      </div>
    </SettingsField>
  {/if}
</section>

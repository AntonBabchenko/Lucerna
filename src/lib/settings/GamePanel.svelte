<script lang="ts">
  // Settings → Game. What the window does when a game starts, the saved-server
  // status permission, and the preferred GPU — three GeneralSettings fields on
  // the one settings contract (`app-settings.svelte.ts`): no value and no live
  // control until the settings are read; a choice patches only its own field;
  // a failed save is said next to the control that failed.
  import {
    type GameStartWindow,
    type GeneralSettings,
    type GpuPreference,
    type GpuStatus,
    commands,
  } from '$lib/ipc/bindings';
  import { describeStoreError, formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import Select from '$lib/ui/Select.svelte';
  import SegmentedControl from '$lib/ui/SegmentedControl.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import {
    appSettings,
    generalDisplayed,
    loadAppSettings,
    patchGeneral,
    saveFailure,
    saveFailureKind,
  } from './app-settings.svelte';
  import SettingsField from './SettingsField.svelte';
  import { onMount } from 'svelte';

  // null while pending or failed: nothing is claimed, every control is disabled.
  const general = $derived(generalDisplayed());
  const loaded = $derived(general !== null);
  const loadError = $derived(
    appSettings.loaded.kind === 'failed' ? appSettings.loaded.error : null,
  );

  async function set<K extends keyof GeneralSettings>(field: K, value: GeneralSettings[K]) {
    await patchGeneral({ [field]: value } as Partial<GeneralSettings>);
  }

  // When a game starts: the backend resolves an old file's checkbox, so a
  // loaded block always carries the field; null means "not read yet" (disabled,
  // nothing pressed) or, defensively, a value this build can't show (usable,
  // nothing claimed).
  const startWindow = $derived<GameStartWindow | null>(general?.game_start_window ?? null);
  const startOptions = $derived([
    { value: 'keep', label: $t('settings.general.playing.keep') },
    { value: 'minimise', label: $t('settings.general.playing.minimise') },
    { value: 'hide_to_tray', label: $t('settings.general.playing.hideToTray') },
  ]);
  const START_HINT: Record<GameStartWindow, TranslationKey> = {
    keep: 'settings.general.playing.keepHint',
    minimise: 'settings.general.playing.minimiseHint',
    hide_to_tray: 'settings.general.playing.trayDescription',
  };
  // The Linux caveat is true only on Linux: some compositors (Wayland) honour a
  // minimise but not the un-minimise. The build says which OS this is.
  let onLinux = $state(false);
  onMount(() => {
    void (async () => {
      try {
        onLinux = (await commands.appBuildInfo()).os === 'linux';
      } catch {
        // Not known: the Linux-only sentence stays hidden.
      }
    })();
  });
  const startDescribedby = $derived(
    [
      startWindow && startWindow !== 'hide_to_tray' ? 'game-start-hint' : null,
      'game-tray-desc',
      onLinux ? 'game-start-linux' : null,
    ]
      .filter(Boolean)
      .join(' '),
  );

  // The consent says the restrictive truth only when it is true: a REFUSED
  // revoke leaves the channel on (the file is unchanged); one the transport
  // lost is decided by the re-read, so it gets the neutral sentence.
  const pingFailure = $derived.by((): string | null => {
    const kind = saveFailureKind('allow_server_ping');
    if (kind === 'refused' && general?.allow_server_ping) {
      return $t('settings.general.serverPing.revokeFailed', {
        error: appSettings.failures.allow_server_ping?.error ?? '',
      });
    }
    return saveFailure('allow_server_ping');
  });

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
  // A stored non-Auto choice keeps acting (where the OS lets it) while the
  // control is hidden; say so, per mechanism, and offer the way back. Nothing
  // is claimed while checking, on Automatic, or without a loaded block.
  const storedPref = $derived(general?.gpu_preference ?? 'auto');
  const storedLabel = $derived(
    $t(
      storedPref === 'high_performance'
        ? 'settings.general.gpu.high'
        : 'settings.general.gpu.power',
    ),
  );
  const storedChoice = $derived.by((): string | null => {
    if (gpu === 'checking' || general === null || storedPref === 'auto') return null;
    if (!('capability' in gpu)) {
      return $t('settings.general.gpu.stored.saved', { label: storedLabel });
    }
    if (gpu.capability.kind === 'available') return null;
    const acts =
      gpu.mechanism === 'windows_registry' ||
      (gpu.mechanism === 'linux_env' && storedPref === 'high_performance');
    if (!acts) return $t('settings.general.gpu.stored.inert', { label: storedLabel });
    return $t(
      gpu.mechanism === 'windows_registry'
        ? 'settings.general.gpu.stored.windows'
        : 'settings.general.gpu.stored.linux',
      { label: storedLabel },
    );
  });

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
    void loadGpu();
  });
</script>

<section class="flex flex-col gap-6">
  {#if loadError !== null}
    <!-- The page-level truth: nothing below is known until a read succeeds. -->
    <div class="flex flex-col gap-2" data-testid="settings-load-failed">
      <StatusMessage
        message={$t('settings.general.loadFailed', { error: loadError })}
        tone="danger"
      />
      <button
        type="button"
        class="btn-secondary btn-sm self-start"
        onclick={() => void loadAppSettings()}
      >
        {$t('settings.general.retryBtn')}
      </button>
    </div>
  {/if}

  <SettingsField anchor="game.tray">
    <div class="flex flex-col gap-3">
      <h3 class="font-medium text-sm text-primary">{$t('settings.general.playing.title')}</h3>
      <div class="flex flex-col gap-1">
        <span class="text-sm text-primary">{$t('settings.general.playing.startWindowLabel')}</span>
        <SegmentedControl
          variant="boxed"
          ariaLabel={$t('settings.general.playing.startWindowLabel')}
          dataTestid="game-start-window"
          describedby={startDescribedby}
          options={startOptions}
          value={startWindow}
          disabled={!loaded}
          onChange={(v) => void set('game_start_window', v as GameStartWindow)}
        />
        <!-- The chosen action's own line, then the tray caveat ALWAYS: activation follows
             focus, so the consequence of Hide to tray must be readable before it is picked. -->
        {#if startWindow && startWindow !== 'hide_to_tray'}
          <p id="game-start-hint" class="text-xs text-muted">{$t(START_HINT[startWindow])}</p>
        {/if}
        <p id="game-tray-desc" class="text-xs text-muted">
          {$t('settings.general.playing.trayDescription')}
        </p>
        {#if onLinux}
          <p id="game-start-linux" class="text-xs text-muted">
            {$t('settings.general.playing.linuxMinimise')}
          </p>
        {/if}
      </div>
      <div data-testid="save-failure-game_start_window">
        <StatusMessage message={saveFailure('game_start_window')} tone="danger" />
      </div>
    </div>
  </SettingsField>

  <!-- The consent gate for the whole server-status feature. Default off, and
       the IP-exposure consequence is stated right here rather than left for the
       user to infer — this is the one place they decide. -->
  <SettingsField anchor="game.serverPing">
    <div class="flex flex-col gap-3">
      <h3 class="font-medium text-sm text-primary">{$t('settings.general.serverPing.title')}</h3>
      <div class="flex flex-col gap-1">
        <label class="flex items-start gap-2 cursor-pointer">
          <input
            type="checkbox"
            class="mt-0.5"
            checked={general?.allow_server_ping ?? false}
            disabled={!loaded}
            aria-describedby="game-server-ping-desc game-server-ping-privacy"
            onchange={(e) => void set('allow_server_ping', e.currentTarget.checked)}
            data-testid="server-ping-toggle"
          />
          <span class="text-sm text-primary">{$t('settings.general.serverPing.label')}</span>
        </label>
        <p id="game-server-ping-desc" class="pl-6 text-xs text-muted">
          {$t('settings.general.serverPing.description')}
        </p>
        <p id="game-server-ping-privacy" class="pl-6 text-xs text-warning-text">
          {$t('settings.general.serverPing.privacy')}
        </p>
      </div>
      <div data-testid="save-failure-allow_server_ping">
        <StatusMessage message={pingFailure} tone="danger" />
      </div>
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
            describedby="game-gpu-note"
            value={general?.gpu_preference ?? null}
            options={gpuOptions}
            disabled={!loaded}
            onChange={(v) => void set('gpu_preference', v as GpuPreference)}
          />
          <span id="game-gpu-note" class="text-xs text-muted" data-testid="gpu-note">{gpuNote}</span
          >
          <div data-testid="save-failure-gpu_preference">
            <StatusMessage message={saveFailure('gpu_preference')} tone="danger" />
          </div>
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
            onclick={() => void set('gpu_preference', 'auto')}
          >
            {$t('settings.general.gpu.resetBtn')}
          </button>
          <div data-testid="save-failure-gpu_preference">
            <StatusMessage message={saveFailure('gpu_preference')} tone="danger" />
          </div>
        {/if}
      </div>
    </SettingsField>
  {/if}
</section>

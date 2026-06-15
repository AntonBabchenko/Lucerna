<script lang="ts">
  // Settings → Updates. Startup-check toggle + manual check / update-now,
  // followed by the "What's new" changelog (moved here from About — it
  // pairs naturally with keeping the app current). Owns only the
  // check_updates_on_startup GeneralSettings field via a fresh RMW.
  import { onMount } from 'svelte';
  import { commands, type GeneralSettings } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { runUpdate, updateInstalling, updateState } from '$lib/update/state.svelte';
  import ChangelogPanel from '$lib/changelog/ChangelogPanel.svelte';
  import { CHANGELOG } from '$lib/changelog/source';

  let general = $state<GeneralSettings>({
    hide_to_tray_during_game: false,
    theme: 'system',
    check_updates_on_startup: true,
    gpu_preference: 'auto',
  });
  let loadError = $state<string | null>(null);
  let saveError = $state<string | null>(null);

  onMount(async () => {
    const r = await commands.appSettingsGet();
    if (r.status === 'ok') general = r.data.general;
    else loadError = formatError(r.error);
  });

  async function save() {
    saveError = null;
    const cur = await commands.appSettingsGet();
    if (cur.status !== 'ok') {
      saveError = formatError(cur.error);
      return;
    }
    const next = { ...cur.data.general, check_updates_on_startup: general.check_updates_on_startup };
    const r = await commands.appSettingsSetGeneral(next);
    if (r.status !== 'ok') saveError = formatError(r.error);
  }

  // Manual update check — mirrors the startup check but reports inline.
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
</script>

<section class="flex flex-col gap-6">
  <div class="flex flex-col gap-3">
    {#if loadError}
      <p class="text-xs text-danger">{loadError}</p>
    {/if}
    <label class="flex items-start gap-2 cursor-pointer">
      <input
        type="checkbox"
        class="mt-0.5"
        bind:checked={general.check_updates_on_startup}
        onchange={() => void save()}
        data-testid="updates-toggle"
      />
      <span class="flex-1">
        <span class="text-sm text-primary">{$t('settings.general.updates.startupLabel')}</span>
        <span class="block text-xs text-muted">
          {$t('settings.general.updates.startupDescription')}
        </span>
      </span>
    </label>
    {#if saveError}
      <p class="text-xs text-danger">{saveError}</p>
    {/if}
    <div class="flex items-center gap-3 flex-wrap">
      <button
        type="button"
        class="btn-secondary btn-sm inline-flex items-center gap-1.5"
        onclick={() => void checkForUpdates()}
        disabled={checking}
        data-testid="check-updates-btn"
      >
        <Icon name="refresh" class="icon-spin-hover" />{checking
          ? $t('settings.general.updates.checking')
          : $t('settings.general.updates.checkBtn')}
      </button>
      {#if checkResult.kind === 'uptodate'}
        <p class="text-xs text-muted" data-testid="update-status">
          {$t('settings.general.updates.uptodate', { version: checkResult.current })}
        </p>
      {:else if checkResult.kind === 'error'}
        <p class="text-xs text-danger" data-testid="update-status">
          {$t('settings.general.updates.error', { message: checkResult.message })}
        </p>
      {:else if checkResult.kind === 'available'}
        <p class="text-xs text-primary" data-testid="update-status">
          {$t('settings.general.updates.available', { version: checkResult.version })}
        </p>
        <button
          type="button"
          class="btn-primary btn-sm"
          onclick={() => void runUpdate()}
          disabled={updateInstalling.value}
          data-testid="update-now-btn"
        >
          {updateInstalling.value
            ? $t('settings.general.updates.installing')
            : $t('settings.general.updates.updateNow')}
        </button>
      {/if}
    </div>
  </div>

  <div class="flex flex-col gap-3 border-t pt-4">
    <h3 class="font-medium text-sm text-primary">{$t('settings.changelog.title')}</h3>
    <ChangelogPanel entries={CHANGELOG} />
  </div>
</section>

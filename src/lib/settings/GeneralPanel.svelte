<script lang="ts">
  // Settings → General. Appearance + playing-time preferences +
  // onboarding replay. Future general settings (language, update
  // prefs) accumulate here per the post-v0.5.0 backlog.
  import { onMount } from 'svelte';
  import {
    commands,
    type ExplanationLevel,
    type GeneralSettings,
    type ThemePreference,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { AVAILABLE_LOCALES, t } from '$lib/i18n';
  import { langPref, setLocalePref } from '$lib/i18n/state.svelte';
  import Select from '$lib/ui/Select.svelte';
  import { explanationState, setExplanationLevel } from '$lib/onboarding/explanation-level.svelte';
  import { replayTour } from '$lib/onboarding/state.svelte';
  import { themeState, setThemePref } from '$lib/theme/state.svelte';
  import { runUpdate, updateInstalling, updateState } from '$lib/update/state.svelte';
  import { settingsOpen } from './state.svelte';

  // Display label per locale code. New community languages fall back to
  // their raw code until a label is added here.
  const LOCALE_LABELS: Record<string, string> = { en: 'English', ru: 'Русский' };

  const languageOptions = $derived([
    { value: 'system', label: $t('settings.general.appearance.languageSystem') },
    ...AVAILABLE_LOCALES.map((code) => ({ value: code, label: LOCALE_LABELS[code] ?? code })),
  ]);

  const tipsOptions = $derived([
    { value: 'basic', label: $t('settings.general.tips.basic') },
    { value: 'advanced', label: $t('settings.general.tips.advanced') },
  ]);

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
    // Fresh read-modify-write. The theme + language pickers persist via
    // their own RMW (setThemePref / setLocalePref) and do NOT update this
    // panel's `general` rune, so writing the whole (stale) `general` here
    // would clobber a just-changed theme/language. Re-read and merge only
    // the fields this panel's toggles own.
    const cur = await commands.appSettingsGet();
    if (cur.status !== 'ok') {
      saveError = formatError(cur.error);
      return;
    }
    const next = {
      ...cur.data.general,
      hide_to_tray_during_game: general.hide_to_tray_during_game,
      check_updates_on_startup: general.check_updates_on_startup,
    };
    const r = await commands.appSettingsSetGeneral(next);
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
    <h3 class="font-medium text-sm text-primary">{$t('settings.general.appearance.title')}</h3>
    <fieldset class="flex flex-col gap-2">
      {#each [{ v: 'system' as ThemePreference, labelKey: 'settings.general.appearance.themeSystem' as const }, { v: 'light' as ThemePreference, labelKey: 'settings.general.appearance.themeLight' as const }, { v: 'dark' as ThemePreference, labelKey: 'settings.general.appearance.themeDark' as const }] as opt (opt.v)}
        <label class="flex items-center gap-2 cursor-pointer">
          <input
            type="radio"
            name="theme"
            value={opt.v}
            checked={themeState.pref === opt.v}
            onchange={() => void setThemePref(opt.v)}
            data-testid="theme-{opt.v}"
          />
          <span class="text-sm">{$t(opt.labelKey)}</span>
        </label>
      {/each}
    </fieldset>
    <div class="flex flex-col gap-1">
      <span class="text-sm text-primary">{$t('settings.general.appearance.language')}</span>
      <Select
        class="text-sm"
        dataTestid="language-select"
        ariaLabel={$t('settings.general.appearance.language')}
        value={langPref.value}
        options={languageOptions}
        onChange={(v) => void setLocalePref(String(v))}
      />
    </div>
  </div>

  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm text-primary">{$t('settings.general.playing.title')}</h3>
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

  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm text-primary">{$t('settings.general.updates.title')}</h3>
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
    <div class="flex items-center gap-3 flex-wrap">
      <button
        type="button"
        class="btn-secondary btn-sm"
        onclick={() => void checkForUpdates()}
        disabled={checking}
        data-testid="check-updates-btn"
      >
        {checking
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

  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm text-primary">{$t('settings.general.tips.title')}</h3>
    <div class="flex flex-col gap-1">
      <span class="text-sm text-primary">{$t('settings.general.tips.levelLabel')}</span>
      <Select
        class="text-sm"
        dataTestid="tip-level-select"
        ariaLabel={$t('settings.general.tips.levelLabel')}
        value={explanationState.level}
        options={tipsOptions}
        onChange={(v) => void setExplanationLevel(v as ExplanationLevel)}
      />
      <span class="text-xs text-muted">{$t('settings.general.tips.levelDescription')}</span>
    </div>
  </div>

  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm text-primary">{$t('settings.general.onboarding.title')}</h3>
    <div class="flex items-center gap-3">
      <button type="button" class="btn-secondary btn-sm" onclick={onReplay}>
        {$t('settings.general.onboarding.replayBtn')}
      </button>
      <p class="text-xs text-muted">{$t('settings.general.onboarding.replayDescription')}</p>
    </div>
  </div>
</section>

<script lang="ts">
  // Settings → Appearance. Theme, interface language, the two icon effects,
  // sidebar buttons. All persist via their own stores (setThemePref /
  // setLocalePref / rainbowFx.set / iconZoomFx.set) — never through
  // appSettingsSetGeneral. Every block opens with the shared h3 recipe
  // (DESIGN §3); the theme picker is a SegmentedControl named by the block
  // heading's text and described by the hint under it.
  import { type ThemePreference } from '$lib/ipc/bindings';
  import { AVAILABLE_LOCALES, t } from '$lib/i18n';
  import { langPref, setLocalePref } from '$lib/i18n/state.svelte';
  import Select from '$lib/ui/Select.svelte';
  import SegmentedControl from '$lib/ui/SegmentedControl.svelte';
  import { themeState, setThemePref } from '$lib/theme/state.svelte';
  import { rainbowFx } from '$lib/fx/rainbow-fx.svelte';
  import { iconZoomFx } from '$lib/fx/icon-zoom-fx.svelte';
  import { SIDEBAR_BUTTONS } from '$lib/layout/sidebar-buttons';
  import { isVisible, setHidden } from '$lib/layout/sidebar-buttons.svelte';
  import { saveFailure } from '$lib/settings/app-settings.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import SettingsField from './SettingsField.svelte';

  const LOCALE_LABELS: Record<string, string> = { en: 'English', ru: 'Русский' };

  const languageOptions = $derived([
    { value: 'system', label: $t('settings.general.appearance.languageSystem') },
    ...AVAILABLE_LOCALES.map((code) => ({ value: code, label: LOCALE_LABELS[code] ?? code })),
  ]);

  // testId per option keeps the `theme-<pref>` ids the tests and the save-failure
  // suite query.
  const themeOptions = $derived([
    {
      value: 'system',
      label: $t('settings.general.appearance.themeSystem'),
      testId: 'theme-system',
    },
    { value: 'light', label: $t('settings.general.appearance.themeLight'), testId: 'theme-light' },
    { value: 'dark', label: $t('settings.general.appearance.themeDark'), testId: 'theme-dark' },
  ]);
</script>

<section class="flex flex-col gap-6">
  <SettingsField anchor="appearance.theme">
    <div class="flex flex-col gap-2">
      <h3 class="font-medium text-sm text-primary">{$t('settings.general.appearance.theme')}</h3>
      <SegmentedControl
        variant="boxed"
        ariaLabel={$t('settings.general.appearance.theme')}
        describedby="appearance-theme-hint"
        options={themeOptions}
        value={themeState.pref}
        onChange={(v) => void setThemePref(v as ThemePreference)}
      />
      <p id="appearance-theme-hint" class="text-xs text-muted">
        {$t('settings.general.appearance.themeHint')}
      </p>
      <!-- A refused save snaps the pick back; this line says why, here. -->
      <div data-testid="save-failure-theme">
        <StatusMessage message={saveFailure('theme')} tone="danger" />
      </div>
    </div>
  </SettingsField>

  <SettingsField anchor="appearance.language">
    <div class="flex flex-col gap-2">
      <h3 class="font-medium text-sm text-primary">{$t('settings.general.appearance.language')}</h3>
      <Select
        class="text-sm"
        dataTestid="language-select"
        ariaLabel={$t('settings.general.appearance.language')}
        value={langPref.value}
        options={languageOptions}
        onChange={(v) => void setLocalePref(String(v))}
      />
      <div data-testid="save-failure-language">
        <StatusMessage message={saveFailure('language')} tone="danger" />
      </div>
    </div>
  </SettingsField>

  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm text-primary">
      {$t('settings.general.appearance.effectsTitle')}
    </h3>
    <SettingsField anchor="appearance.rainbowIcons">
      <label class="flex items-start gap-2 cursor-pointer">
        <input
          type="checkbox"
          class="mt-0.5"
          checked={rainbowFx.enabled}
          onchange={(e) => rainbowFx.set(e.currentTarget.checked)}
          data-testid="rainbow-icons-toggle"
        />
        <span class="flex-1">
          <span class="text-sm text-primary">{$t('settings.general.appearance.rainbowIcons')}</span>
          <span class="block text-xs text-muted">
            {$t('settings.general.appearance.rainbowIconsDescription')}
          </span>
        </span>
      </label>
    </SettingsField>

    <SettingsField anchor="appearance.iconZoom">
      <label class="flex items-start gap-2 cursor-pointer">
        <input
          type="checkbox"
          class="mt-0.5"
          checked={iconZoomFx.enabled}
          onchange={(e) => iconZoomFx.set(e.currentTarget.checked)}
          data-testid="icon-zoom-toggle"
        />
        <span class="flex-1">
          <span class="text-sm text-primary">{$t('settings.general.appearance.iconZoom')}</span>
          <span class="block text-xs text-muted">
            {$t('settings.general.appearance.iconZoomDescription')}
          </span>
        </span>
      </label>
    </SettingsField>
  </div>

  <SettingsField anchor="appearance.sidebarButtons">
    <!-- A fieldset whose legend wraps the h3: each box reads "Mods, checkbox —
         Sidebar buttons" and the block stays in heading navigation (10b §10). -->
    <fieldset class="flex flex-col gap-2">
      <legend class="mb-1">
        <h3 class="font-medium text-sm text-primary">
          {$t('settings.general.appearance.sidebarButtons.title')}
        </h3>
      </legend>
      <p class="text-xs text-muted">
        {$t('settings.general.appearance.sidebarButtons.description')}
      </p>
      {#each SIDEBAR_BUTTONS as b (b.id)}
        <label class="flex items-center gap-2 cursor-pointer">
          <input
            type="checkbox"
            checked={isVisible(b.id)}
            onchange={(e) => void setHidden(b.id, !e.currentTarget.checked)}
            data-testid="sidebar-button-toggle-{b.id}"
          />
          <span class="text-sm text-primary">{$t(b.labelKey)}</span>
        </label>
      {/each}
    </fieldset>
  </SettingsField>
</section>

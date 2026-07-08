<script lang="ts">
  // Settings → Appearance. Theme, interface language, rainbow icon
  // animation, icon hover-zoom. All persist via their own stores
  // (setThemePref / setLocalePref / rainbowFx.set / iconZoomFx.set) —
  // never through appSettingsSetGeneral.
  import { type ThemePreference } from '$lib/ipc/bindings';
  import { AVAILABLE_LOCALES, t } from '$lib/i18n';
  import { langPref, setLocalePref } from '$lib/i18n/state.svelte';
  import Select from '$lib/ui/Select.svelte';
  import { themeState, setThemePref } from '$lib/theme/state.svelte';
  import { rainbowFx } from '$lib/fx/rainbow-fx.svelte';
  import { iconZoomFx } from '$lib/fx/icon-zoom-fx.svelte';
  import { SIDEBAR_BUTTONS } from '$lib/layout/sidebar-buttons';
  import { isVisible, setHidden } from '$lib/layout/sidebar-buttons.svelte';

  const LOCALE_LABELS: Record<string, string> = { en: 'English', ru: 'Русский' };

  const languageOptions = $derived([
    { value: 'system', label: $t('settings.general.appearance.languageSystem') },
    ...AVAILABLE_LOCALES.map((code) => ({ value: code, label: LOCALE_LABELS[code] ?? code })),
  ]);
</script>

<section class="flex flex-col gap-4">
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

  <div class="border-t border-border-subtle pt-4 mt-2 flex flex-col gap-2">
    <div>
      <span class="text-sm text-primary">
        {$t('settings.general.appearance.sidebarButtons.title')}
      </span>
      <span class="block text-xs text-muted">
        {$t('settings.general.appearance.sidebarButtons.description')}
      </span>
    </div>
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
  </div>
</section>

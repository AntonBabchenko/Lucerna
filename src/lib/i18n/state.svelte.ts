// Locale runtime — bridges the persisted preference, the svelte-i18n
// `locale` store, and localStorage (anti-FOUC). Mirrors theme/state.
//
//   langPref.value      — 'system' | 'en' | 'ru' | … the saved choice.
//   resolved locale     — what svelte-i18n is actually displaying.
import { commands } from '$lib/ipc/bindings';
import { AVAILABLE_LOCALES, locale } from './index';
import { LOCALE_STORAGE_KEY, getOsLang, resolveLocale } from './resolve';

export const langPref = $state<{ value: string }>({ value: 'system' });

function mirror(pref: string) {
  try {
    localStorage.setItem(LOCALE_STORAGE_KEY, pref);
  } catch {
    /* private mode etc. — degrade silently */
  }
}

/**
 * Called once at app start with the persisted pref from app.json.
 * Reconciles the rune + svelte-i18n locale + localStorage.
 */
export function initLocale(initial: string): void {
  langPref.value = initial;
  mirror(initial);
  locale.set(resolveLocale(initial, AVAILABLE_LOCALES, getOsLang()));
}

/**
 * Called from the Settings picker. Switches the live locale instantly
 * (no reload) and persists to app.json via app_settings_set_general.
 */
export async function setLocalePref(pref: string): Promise<void> {
  langPref.value = pref;
  mirror(pref);
  locale.set(resolveLocale(pref, AVAILABLE_LOCALES, getOsLang()));
  const get = await commands.appSettingsGet();
  if (get.status !== 'ok') return;
  const next = { ...get.data.general, language: pref };
  await commands.appSettingsSetGeneral(next);
}

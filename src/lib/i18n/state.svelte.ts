// Locale runtime — bridges the persisted preference, the svelte-i18n
// `locale` store, and localStorage (anti-FOUC). Mirrors theme/state.
//
//   langPref.value      — 'system' | 'en' | 'ru' | … the saved choice.
//   resolved locale     — what svelte-i18n is actually displaying.
import { commands } from '$lib/ipc/bindings';
import { AVAILABLE_LOCALES, locale } from './index';
import { getOsLang, LOCALE_STORAGE_KEY, resolveLocale } from './resolve';

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

/** Apply a preference to the three places the live UI reads it from. Shared by
 *  the user-driven set and its rollback so the two can never diverge. */
function applyPref(pref: string): void {
  langPref.value = pref;
  mirror(pref);
  locale.set(resolveLocale(pref, AVAILABLE_LOCALES, getOsLang()));
}

/**
 * Called from the Settings picker. Switches the live locale instantly
 * (no reload) and persists to app.json via app_settings_set_general.
 * If either the settings read or the write fails, roll back — including the
 * localStorage mirror, which is what the next launch's anti-FOUC path reads,
 * so leaving it set would make the UI open in a language app.json does not
 * name. Same contract as `setExplanationLevel` / `setHidden`.
 */
export async function setLocalePref(pref: string): Promise<void> {
  const prev = langPref.value;
  applyPref(pref);
  const get = await commands.appSettingsGet();
  if (get.status !== 'ok') {
    applyPref(prev);
    return;
  }
  const next = { ...get.data.general, language: pref };
  const res = await commands.appSettingsSetGeneral(next);
  // A newer pick may have landed while this write was in flight; rolling back
  // then would clobber it. Only the choice still on screen may be reverted —
  // the same post-await re-check LogsPopover.loadContent makes before every
  // state commit.
  if (res.status !== 'ok' && langPref.value === pref) applyPref(prev);
}

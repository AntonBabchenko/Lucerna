// Single import surface for i18n. At module load it (1) registers every
// bundled dictionary (dictionaries.ts) synchronously (no async loader → no
// flash-of-keys), and (2) calls svelte-i18n init() so the `$t` / `$_` stores
// are usable on first paint. The boot sequence later reconciles the actual
// persisted preference via initLocale().
import { addMessages, init, locale } from 'svelte-i18n';
import { AVAILABLE_LOCALES, BUNDLED_LOCALES, SYSTEM_LOCALES } from './dictionaries';
import { FALLBACK_LOCALE, getOsLang, readStoredPref, resolveLocale } from './resolve';

// LocaleDictionary is declared but not publicly exported by svelte-i18n, so
// the dictionary is cast to addMessages()'s own parameter type.
for (const { code, dict } of BUNDLED_LOCALES) {
  addMessages(code, dict as Parameters<typeof addMessages>[1]);
}

// Initialise immediately with the best guess from localStorage + OS, so
// components that subscribe to $t during first render get real strings.
init({
  fallbackLocale: FALLBACK_LOCALE,
  initialLocale: resolveLocale(readStoredPref(), AVAILABLE_LOCALES, getOsLang(), SYSTEM_LOCALES),
});

export { type Translate, t } from './typed';
export { AVAILABLE_LOCALES, locale };
